use super::*;
use crate::provider::adapter::MockAdapter;
use rho_harness_core::model::ToolFunction;

#[derive(Default)]
struct TestStepBuffers {
    current_messages: Vec<ChatMessage>,
    new_turn_messages: Vec<ChatMessage>,
    request_count: usize,
    all_completion_calls: Vec<CompletionCall>,
    total_usage: StructuralUsage,
}

impl<'a> RunnerStepContext<'a> {
    fn from_test_buffers(buf: &'a mut TestStepBuffers, tracker: &'a mut RepeatedCallTracker) -> Self {
        Self {
            current_messages: &mut buf.current_messages,
            new_turn_messages: &mut buf.new_turn_messages,
            repeat_tracker: tracker,
            request_count: &mut buf.request_count,
            all_completion_calls: &mut buf.all_completion_calls,
            total_usage: &mut buf.total_usage,
        }
    }
}

fn test_agent_env(
    model: Arc<crate::provider::adapter::MockAdapter>,
) -> (AgentRunner, tempfile::TempDir, TestStepBuffers, RepeatedCallTracker) {
    test_agent_env_with(model, Arc::new(crate::engine::eval::presenter::NoopPresenter))
}

fn test_agent_env_with(
    model: Arc<crate::provider::adapter::MockAdapter>,
    pres: Arc<dyn rho_harness_core::presentation::Presenter>,
) -> (AgentRunner, tempfile::TempDir, TestStepBuffers, RepeatedCallTracker) {
    let temp = tempfile::tempdir().unwrap();
    let sm = rho_harness_core::session::SessionManager::new(temp.path(), None).unwrap();
    let cfg = crate::engine::runner::sink::TerminalSinkConfig {
        model_label: "test".into(),
        run_tracker: Default::default(),
    };
    let sink = TerminalApprovalSink::new(&pres, cfg, sm.clone());
    let inbox = crate::mailbox::Mailboxes::default().open(&sm.session_id, None);
    let runner = AgentRunner::new(model, Vec::new(), sm, sink).inbox(Some(inbox));
    (
        runner,
        temp,
        TestStepBuffers::default(),
        RepeatedCallTracker::new(std::path::PathBuf::from(".")),
    )
}

use crate::engine::runner::turn::types::BACKGROUND_QUEUE_TEST_LOCK;

/// Posts a system notice to agent `to` through the test runner's mailbox registry.
fn notify(runner: &AgentRunner, to: &str, body: &str) {
    let inbox = runner.inbox.as_ref().expect("test runners have a mailbox");
    inbox
        .registry()
        .send(crate::mailbox::Envelope::new(
            crate::mailbox::Party::System,
            to,
            crate::mailbox::Kind::Event,
            body,
        ))
        .unwrap();
}

fn acking_emitter() -> TurnEmitter {
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    tokio::spawn(async move {
        while let Some(item) = rx.recv().await {
            if let Ok(MultiTurnStreamItem::DisplayBarrier(drained)) = item {
                drained.notify_one();
            }
        }
    });
    TurnEmitter { tx, cancellation: None }
}

#[tokio::test]
async fn tool_dispatch_waits_until_queued_reasoning_is_displayed() {
    use rho_harness_core::presentation::{RecordingSink, StructuredPresenter, UiEvent};
    let model = Arc::new(MockAdapter::from_turns(vec![
        vec![
            ModelStreamEvent::Reasoning("The candidate".into()),
            ModelStreamEvent::Reasoning(" list comes from the engine.".into()),
            ModelStreamEvent::ToolCall(ToolCall::new(
                "call-1",
                ToolFunction::new("read", serde_json::json!({"path": "a.txt"})),
            )),
        ],
        vec![ModelStreamEvent::Text("done".into())],
    ]));
    let recording = RecordingSink::new();
    let (runner, _temp, _, _) = test_agent_env_with(model, Arc::new(StructuredPresenter::recording(recording.clone())));
    let sink = Arc::clone(&runner.sink);
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let emitter = TurnEmitter { tx, cancellation: None };
    let producer = async move { run_agent_loop(runner, &emitter).await };
    let consumer = async move {
        while let Some(Ok(item)) = rx.recv().await {
            match item {
                MultiTurnStreamItem::StreamAssistantItem(StreamEvent::Reasoning(text)) => sink.emit_reasoning(&text),
                MultiTurnStreamItem::DisplayBarrier(drained) => drained.notify_one(),
                _ => {}
            }
        }
    };

    let (result, ()) = tokio::join!(producer, consumer);

    result.unwrap();
    let events = recording.events();
    let tool_started = events
        .iter()
        .position(|e| matches!(e, UiEvent::ToolStarted { .. }))
        .expect("tool start recorded");
    let thinking: Vec<&str> = events[..tool_started]
        .iter()
        .filter_map(|e| match e {
            UiEvent::ThinkingToken { token } => Some(token.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(thinking, ["The candidate", " list comes from the engine."]);
}

#[tokio::test]
async fn test_wait_for_background_jobs_flow() {
    let _queue = BACKGROUND_QUEUE_TEST_LOCK.lock().await;
    let (runner, _temp, mut buf, mut tracker) = test_agent_env(Arc::new(MockAdapter::default()));
    let sig = CancellationSignal::default();
    let runner = runner.cancellation(Some(sig.clone()));
    let owner = runner.session_file().session_id.clone();
    let mut ctx = RunnerStepContext::from_test_buffers(&mut buf, &mut tracker);
    let emitter = acking_emitter();

    assert!(!wait_for_background_jobs(&runner, &mut ctx, &emitter).await.unwrap());
    let registry = runner.inbox.as_ref().map(|inbox| inbox.registry().clone());
    let job = crate::tools::jobs::JobManager::global().start(&owner, registry, "bg-1".into(), "echo 1".into());
    tokio::spawn(async move {
        job.finish(
            crate::tools::jobs::JobStatus::Completed {
                exit_code: 0,
                output: "ok".into(),
            },
            Some("<system-notice>done</system-notice>".into()),
        );
    });
    assert!(wait_for_background_jobs(&runner, &mut ctx, &emitter).await.unwrap());
    crate::tools::jobs::JobManager::global().register(&owner, "bg-2".into(), "sleep".into());
    sig.cancel();
    assert!(matches!(
        wait_for_background_jobs(&runner, &mut ctx, &emitter).await,
        Err(AppError::Cancelled(_))
    ));
    crate::tools::jobs::JobManager::global()
        .update_status("bg-2", crate::tools::jobs::JobStatus::Failed { error: "a".into() });
}

#[tokio::test]
async fn a_turn_neither_waits_for_nor_receives_another_sessions_background_work() {
    let _queue = BACKGROUND_QUEUE_TEST_LOCK.lock().await;
    let (runner, _temp, mut buf, mut tracker) = test_agent_env(Arc::new(MockAdapter::default()));
    let owner = runner.session_file().session_id.clone();
    let mut ctx = RunnerStepContext::from_test_buffers(&mut buf, &mut tracker);
    let emitter = acking_emitter();
    let jobs = crate::tools::jobs::JobManager::global();
    let elsewhere = jobs.register("other-session-3f1", "other-job-3f1".into(), "sleep".into());
    let other_inbox = runner
        .inbox
        .as_ref()
        .unwrap()
        .registry()
        .open("other-session-3f1", None);
    notify(&runner, "other-session-3f1", "other-notice-3f1");
    notify(&runner, &owner, "own-notice-3f1");

    let delivered = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        wait_for_background_jobs(&runner, &mut ctx, &emitter),
    )
    .await
    .expect("the turn must not wait for another session's running job")
    .unwrap();

    assert!(delivered);
    let messages = serde_json::to_string(&*ctx.current_messages).unwrap();
    assert!(messages.contains("own-notice-3f1"), "{messages}");
    assert!(!messages.contains("other-notice-3f1"), "{messages}");
    assert!(!elsewhere.is_cancelled(), "another session's work is left alone");
    assert!(jobs.has_running("other-session-3f1"));
    assert_eq!(other_inbox.take()[0].body, "other-notice-3f1");
    jobs.update_status(
        "other-job-3f1",
        crate::tools::jobs::JobStatus::Failed { error: "done".into() },
    );
}

#[tokio::test]
async fn a_job_that_finished_before_the_turn_ended_still_delivers_its_notification() {
    let _queue = BACKGROUND_QUEUE_TEST_LOCK.lock().await;
    let (runner, _temp, mut buf, mut tracker) = test_agent_env(Arc::new(MockAdapter::default()));
    let mut ctx = RunnerStepContext::from_test_buffers(&mut buf, &mut tracker);
    let emitter = acking_emitter();
    notify(&runner, &runner.session_file().session_id.clone(), "finished-early-4c2");

    assert!(wait_for_background_jobs(&runner, &mut ctx, &emitter).await.unwrap());
    assert!(
        serde_json::to_string(&*ctx.current_messages)
            .unwrap()
            .contains("finished-early-4c2")
    );
}

struct RendezvousTool {
    name: &'static str,
    safe: bool,
    barrier: Arc<tokio::sync::Barrier>,
}

#[async_trait::async_trait]
impl EngineTool for RendezvousTool {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        "waits for its peer"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    async fn execute(&self, _args: serde_json::Value) -> Result<crate::tools::ToolResult, AppError> {
        self.barrier.wait().await;
        Ok(crate::tools::ToolResult::success(format!("{} done", self.name)))
    }
    fn concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        self.safe
    }
}

struct ScriptedTool {
    name: &'static str,
    result: crate::tools::ToolResult,
}

#[async_trait::async_trait]
impl EngineTool for ScriptedTool {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        "returns a fixed result"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    async fn execute(&self, _args: serde_json::Value) -> Result<crate::tools::ToolResult, AppError> {
        Ok(self.result.clone())
    }
}

#[tokio::test]
async fn a_tool_can_end_the_turn_once_its_batch_is_answered() {
    let _queue = BACKGROUND_QUEUE_TEST_LOCK.lock().await;
    let model = Arc::new(MockAdapter::from_turns(vec![
        vec![
            ModelStreamEvent::Text("asking".into()),
            ModelStreamEvent::ToolCall(ToolCall::new("c1", ToolFunction::new("pause", serde_json::json!({})))),
        ],
        vec![ModelStreamEvent::Text("never reached".into())],
    ]));
    let (runner, _temp, _, _) = test_agent_env(model.clone());
    let pause = crate::tools::ToolResult {
        ends_turn: true,
        ..crate::tools::ToolResult::success("question sent")
    };
    let store = crate::engine::runner::turn::MemoryStore::new("pauser", Vec::new());
    let runner = AgentRunner {
        tools: vec![Arc::new(ScriptedTool {
            name: "pause",
            result: pause,
        })],
        ..runner
    }
    .store(Arc::new(store.clone()))
    .prompt("go");

    let mut stream = runner.stream();
    let mut final_output = None;
    while let Some(item) = stream.next().await {
        match item.unwrap() {
            MultiTurnStreamItem::DisplayBarrier(drained) => drained.notify_one(),
            MultiTurnStreamItem::FinalResponse(resp) => final_output = Some(resp.output),
            _ => {}
        }
    }

    assert_eq!(model.requests().len(), 1, "the turn ended after the batch");
    assert_eq!(final_output.as_deref(), Some("asking"));
    let messages = store.messages();
    assert_eq!(messages.len(), 3, "prompt, call, and its answer are kept: {messages:?}");
    assert_eq!(result_text(&messages[2]), "question sent");
}

#[derive(Default)]
struct SeenCalls(std::sync::Mutex<Vec<String>>);

impl ToolObserver for SeenCalls {
    fn denied(&self, call: &ToolCall, reason: &str) {
        self.0
            .lock()
            .unwrap()
            .push(format!("denied {}: {reason}", call.function.name));
    }
    fn finished(&self, call: &ToolCall, _args: &serde_json::Value, output: &str, is_error: bool) {
        self.0
            .lock()
            .unwrap()
            .push(format!("finished {}: {output} (error: {is_error})", call.function.name));
    }
}

#[tokio::test]
async fn the_tool_observer_hears_refused_and_finished_calls() {
    let (runner, _temp, _, mut tracker) = test_agent_env(Arc::new(MockAdapter::default()));
    let scope = crate::permission::policy::parse_scope_from_str("[permission.bash]\n\"rm *\" = \"deny\"\n").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let hook = Arc::new(crate::permission::PermissionHook::with_policy(
        Some(dir.path().to_path_buf()),
        Arc::new(crate::permission::tests::mock::MockHookPresenter::new(false, None)),
        crate::permission::policy::build_policy(None, Some(scope)),
    ));
    let seen = Arc::new(SeenCalls::default());
    let observer: Arc<dyn ToolObserver> = seen.clone();
    let refused = ToolCall::new(
        "c1",
        ToolFunction::new("bash", serde_json::json!({ "command": "rm x" })),
    );
    let ran = ToolCall::new("c2", ToolFunction::new("echo", serde_json::json!({})));
    let tools: Vec<Arc<dyn EngineTool>> = vec![Arc::new(ScriptedTool {
        name: "echo",
        result: crate::tools::ToolResult::success("hi"),
    })];
    let mut ctx = ToolExecutionContext {
        tools: &tools,
        sink: &runner.sink,
        repeat_tracker: &mut tracker,
        lifecycle: None,
        permission: Some(&hook),
        observer: Some(&observer),
    };

    let (message, control) = execute_single_tool_call(&refused, &mut ctx).await;
    ctx.permission = None;
    execute_single_tool_call(&ran, &mut ctx).await;

    assert_eq!(control, CallControl::Continue);
    let seen = seen.0.lock().unwrap();
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert_eq!(seen[0], format!("denied bash: {}", result_text(&message)));
    assert!(seen[0].starts_with("denied bash: Permission denied"), "{seen:?}");
    assert_eq!(seen[1], "finished echo: hi (error: false)");
}

fn rendezvous_tools(safe: bool) -> Vec<Arc<dyn EngineTool>> {
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    ["alpha", "beta"]
        .into_iter()
        .map(|name| {
            Arc::new(RendezvousTool {
                name,
                safe,
                barrier: barrier.clone(),
            }) as Arc<dyn EngineTool>
        })
        .collect()
}

fn calls_to(names: &[&str]) -> Vec<ToolCall> {
    names
        .iter()
        .enumerate()
        .map(|(i, n)| ToolCall::new(format!("call-{i}"), ToolFunction::new(*n, serde_json::json!({"i": i}))))
        .collect()
}

fn result_text(message: &ChatMessage) -> String {
    match message {
        ChatMessage::User { content } => match &content[0] {
            rho_harness_core::model::UserContent::ToolResult(tr) => match &tr.content[0] {
                rho_harness_core::model::ToolResultContent::Text(t) => t.text.clone(),
                _ => String::new(),
            },
            _ => String::new(),
        },
        _ => String::new(),
    }
}

#[tokio::test]
async fn permission_checks_see_json_encoded_arguments_decoded() {
    let dir = tempfile::tempdir().unwrap();
    let presenter = Arc::new(crate::permission::tests::mock::MockHookPresenter::new(false, None));
    let policy = crate::permission::policy::build_policy(None, None);
    let hook = Arc::new(crate::permission::PermissionHook::with_policy(
        Some(dir.path().to_path_buf()),
        presenter,
        policy,
    ));
    let encoded = serde_json::json!("{\"path\": \"/etc/passwd\"}");
    let call = ToolCall::new("call-0", ToolFunction::new("read", encoded));

    let decision = evaluate_tool_dispatch(&call, None, Some(&hook)).await;

    assert!(
        matches!(decision, DispatchDecision::Deny(_)),
        "outside-workspace read must not proceed"
    );
}

#[tokio::test]
async fn concurrency_safe_batches_run_together_and_keep_call_order() {
    let (runner, _temp, _, mut tracker) = test_agent_env(Arc::new(MockAdapter::default()));
    let tools = rendezvous_tools(true);
    let calls = calls_to(&["alpha", "beta"]);
    assert!(batch_is_concurrency_safe(&tools, &calls));
    let mut ctx = ToolExecutionContext {
        tools: &tools,
        sink: &runner.sink,
        repeat_tracker: &mut tracker,
        lifecycle: None,
        permission: None,
        observer: None,
    };

    let outcomes = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        execute_concurrent_tool_calls(&calls, &mut ctx),
    )
    .await
    .expect("calls must overlap; a sequential run would deadlock on the barrier");

    let texts: Vec<String> = outcomes.iter().map(|(m, _)| result_text(m)).collect();
    assert_eq!(texts, vec!["alpha done", "beta done"]);
    assert!(outcomes.iter().all(|(_, control)| *control == CallControl::Continue));
}

#[test]
fn batches_with_unsafe_unknown_or_single_calls_stay_sequential() {
    let unsafe_tools = rendezvous_tools(false);
    let safe_tools = rendezvous_tools(true);
    assert!(!batch_is_concurrency_safe(&unsafe_tools, &calls_to(&["alpha", "beta"])));
    assert!(!batch_is_concurrency_safe(&safe_tools, &calls_to(&["alpha"])));
    assert!(!batch_is_concurrency_safe(
        &safe_tools,
        &calls_to(&["alpha", "missing"])
    ));
    assert!(batch_is_concurrency_safe(&safe_tools, &calls_to(&["alpha", "beta"])));
}

#[tokio::test]
async fn concurrent_batch_repeats_are_settled_without_running_the_tool() {
    let (runner, _temp, _, mut tracker) = test_agent_env(Arc::new(MockAdapter::default()));
    let barrier = Arc::new(tokio::sync::Barrier::new(1));
    let tools: Vec<Arc<dyn EngineTool>> = vec![Arc::new(RendezvousTool {
        name: "alpha",
        safe: true,
        barrier,
    })];
    let same = ToolCall::new("c", ToolFunction::new("alpha", serde_json::json!({"same": true})));
    let calls = vec![same.clone(); 12];
    let mut ctx = ToolExecutionContext {
        tools: &tools,
        sink: &runner.sink,
        repeat_tracker: &mut tracker,
        lifecycle: None,
        permission: None,
        observer: None,
    };

    let outcomes = execute_concurrent_tool_calls(&calls, &mut ctx).await;

    assert_eq!(outcomes.len(), 12);
    assert!(outcomes.iter().any(|(m, _)| result_text(m) != "alpha done"));
}

#[tokio::test]
async fn agent_loop_stops_calling_model_once_receiver_is_dropped() {
    let model = Arc::new(MockAdapter::from_turns(vec![
        vec![ModelStreamEvent::ToolCall(ToolCall::new(
            "call-1",
            ToolFunction::new("read", serde_json::json!({"path": "a.txt"})),
        ))],
        vec![ModelStreamEvent::Text("unreachable".to_string())],
    ]));
    let (runner, _temp, _, _) = test_agent_env(model.clone());
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let emitter = TurnEmitter { tx, cancellation: None };
    let consumer = async move {
        let first = rx.recv().await;
        assert!(matches!(
            first,
            Some(Ok(MultiTurnStreamItem::StreamAssistantItem(
                StreamEvent::Arguments { .. }
            )))
        ));
        drop(rx);
    };

    let (result, ()) = tokio::join!(run_agent_loop(runner.clone(), &emitter), consumer);

    assert!(matches!(result, Err(AppError::Cancelled(_))));
    assert_eq!(model.requests().len(), 1);
    assert!(runner.session_file().active_messages().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_turn_is_reminded_once_about_worktrees_its_session_still_owns() {
    let model = Arc::new(MockAdapter::from_turns(vec![
        vec![crate::provider::adapter::ModelStreamEvent::Text("done".into())],
        vec![crate::provider::adapter::ModelStreamEvent::Text(
            "kept for review".into(),
        )],
    ]));
    let (runner, _temp, _, _) = test_agent_env(model.clone());
    let (dir, root) = crate::worktree::git::test_repo::create(&[("a.rs", "base\n")]);
    let store = crate::worktree::WorktreeStore::new(dir.path().join("store"));
    let session = runner.session_file().session_id.clone();
    let mine = store
        .create(&root, "subagent fix", Some(&session))
        .await
        .unwrap()
        .unwrap();
    store
        .create(&root, "other session", Some("other"))
        .await
        .unwrap()
        .unwrap();
    let runner = runner.worktrees(Some(store)).working_dir(root);

    let emitter = acking_emitter();
    let result = run_agent_loop(runner, &emitter).await;

    assert!(result.is_ok(), "{result:?}");
    let requests = model.requests();
    assert_eq!(requests.len(), 2, "one reminder, then the turn ends");
    let reminder = requests[1]
        .messages
        .last()
        .and_then(ChatMessage::text)
        .unwrap_or_default()
        .to_string();
    assert!(
        reminder.starts_with("[Before finishing: this session still owns unmerged agent worktrees: "),
        "{reminder}"
    );
    assert!(
        reminder.contains(&format!("`{}` (subagent fix)", mine.id)),
        "{reminder}"
    );
    assert!(!reminder.contains("other session"), "{reminder}");
}

#[tokio::test]
async fn agent_loop_does_not_call_model_after_cancellation() {
    let model = Arc::new(MockAdapter::text("unreachable"));
    let (runner, _temp, _, _) = test_agent_env(model.clone());
    let cancellation = CancellationSignal::default();
    cancellation.cancel();
    let runner = runner.cancellation(Some(cancellation));
    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    let emitter = TurnEmitter {
        tx,
        cancellation: runner.cancellation.clone(),
    };

    let result = run_agent_loop(runner, &emitter).await;

    assert!(matches!(result, Err(AppError::Cancelled(_))));
    assert!(model.requests().is_empty());
}

#[tokio::test]
async fn agent_loop_reports_failed_commit_instead_of_completing() {
    let model = Arc::new(MockAdapter::text("answer with sk-live-secret-42"));
    let (runner, _temp, _, _) = test_agent_env(model);
    runner
        .session_file()
        .add_secrets_async(["sk-live-secret-42".to_string()])
        .await
        .unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let emitter = TurnEmitter { tx, cancellation: None };

    let result = run_agent_loop(runner, &emitter).await;
    drop(emitter);

    assert!(result.is_err());
    while let Some(item) = rx.recv().await {
        assert!(!matches!(item, Ok(MultiTurnStreamItem::FinalResponse(_))));
    }
}

fn tool_then(turns: Vec<Vec<ModelStreamEvent>>) -> Arc<MockAdapter> {
    let mut all = vec![vec![
        ModelStreamEvent::ReasoningBlock(Reasoning::new_with_signature("plan", Some("sig-1".into()))),
        ModelStreamEvent::ToolCall(ToolCall::new(
            "call-1",
            ToolFunction::new("missing", serde_json::json!({})),
        )),
    ]];
    all.extend(turns);
    Arc::new(MockAdapter::from_turns(all))
}

async fn run_prompt(model: Arc<MockAdapter>) -> (AgentRunner, Result<(), AppError>, tempfile::TempDir) {
    let (runner, temp, _, _) = test_agent_env(model);
    let runner = runner.prompt("do it");
    let emitter = acking_emitter();
    let result = run_agent_loop(runner.clone(), &emitter).await;
    (runner, result, temp)
}

fn has_signed_reasoning(messages: &[ChatMessage]) -> bool {
    messages.iter().any(|m| match m {
        ChatMessage::Assistant { content, .. } => content
            .iter()
            .any(|c| matches!(c, AssistantContent::Reasoning(r) if r.first_signature() == Some("sig-1"))),
        _ => false,
    })
}

#[tokio::test]
async fn tool_loop_replays_signed_reasoning_and_nudges_once_after_empty_completion() {
    let model = tool_then(vec![
        vec![ModelStreamEvent::Finish(FinishReason::Stop)],
        vec![ModelStreamEvent::Text("done".into())],
    ]);

    let (runner, result, _temp) = run_prompt(model.clone()).await;

    result.unwrap();
    let requests = model.requests();
    assert!(has_signed_reasoning(&requests[1].messages));
    assert_eq!(
        requests[2].messages.last(),
        Some(&ChatMessage::user(EMPTY_COMPLETION_NUDGE))
    );
    let history = runner.session_file().active_messages().await.unwrap();
    assert!(has_signed_reasoning(&history));
    assert_eq!(history.last(), Some(&ChatMessage::assistant("done")));
}

#[tokio::test]
async fn repeated_empty_completion_keeps_tool_work_as_checkpoint() {
    let model = tool_then(vec![
        vec![ModelStreamEvent::Finish(FinishReason::Stop)],
        vec![ModelStreamEvent::Finish(FinishReason::Stop)],
    ]);

    let (runner, result, _temp) = run_prompt(model).await;

    assert!(matches!(&result, Err(AppError::Provider(m)) if m.contains("empty response")));
    let checkpoint = runner.session_file().load_checkpoint().await.unwrap().unwrap();
    assert_eq!(checkpoint.len(), 4);
    assert_eq!(checkpoint.last(), Some(&ChatMessage::user(EMPTY_COMPLETION_NUDGE)));
    runner
        .session_file()
        .promote_checkpoint(vec![ChatMessage::user("next"), ChatMessage::assistant("done")])
        .await
        .unwrap();
}

fn tool_turn_then(failures: Vec<Vec<Result<ModelStreamEvent, AppError>>>) -> Arc<MockAdapter> {
    let mut turns = vec![vec![
        Ok(ModelStreamEvent::ReasoningBlock(Reasoning::new_with_signature(
            "plan",
            Some("sig-1".into()),
        ))),
        Ok(ModelStreamEvent::ToolCall(ToolCall::new(
            "call-1",
            ToolFunction::new("missing", serde_json::json!({})),
        ))),
    ]];
    turns.extend(failures);
    Arc::new(MockAdapter::from_result_turns(turns))
}

fn dropped() -> Vec<Result<ModelStreamEvent, AppError>> {
    vec![Err(AppError::Network(
        "Stream transport failed: connection reset".into(),
    ))]
}

#[tokio::test(start_paused = true)]
async fn failed_turns_keep_completed_tool_work_as_checkpoint() {
    let cases = [
        ("retries exhausted", vec![dropped(), dropped(), dropped()], 4),
        (
            "not retryable",
            vec![vec![Err(AppError::Provider("invalid request".into()))]],
            2,
        ),
    ];
    for (name, failures, expected_requests) in cases {
        let model = tool_turn_then(failures);

        let (runner, result, _temp) = run_prompt(Arc::clone(&model)).await;

        assert!(result.is_err(), "{name}");
        assert_eq!(model.requests().len(), expected_requests, "{name}");
        let checkpoint = runner
            .session_file()
            .load_checkpoint()
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("{name}: tool work from the failed turn must be checkpointed"));
        assert_eq!(checkpoint.first(), Some(&ChatMessage::user("do it")), "{name}");
        assert!(has_signed_reasoning(&checkpoint), "{name}");
        assert!(
            matches!(checkpoint.last(), Some(ChatMessage::User { content })
                if content.iter().any(|c| matches!(c, rho_harness_core::model::UserContent::ToolResult(_)))),
            "{name}: {checkpoint:?}"
        );
    }
}

#[tokio::test]
async fn failures_before_any_model_output_leave_no_checkpoint() {
    let model = Arc::new(MockAdapter::from_result_turns(vec![vec![Err(AppError::Provider(
        "invalid request".into(),
    ))]]));

    let (runner, result, _temp) = run_prompt(model).await;

    assert!(result.is_err());
    assert!(runner.session_file().load_checkpoint().await.unwrap().is_none());
}

#[derive(Debug)]
struct HangsAfterScript(MockAdapter);

#[async_trait::async_trait]
impl crate::provider::adapter::ModelAdapter for HangsAfterScript {
    fn model_name(&self) -> &str {
        self.0.model_name()
    }

    fn provider_name(&self) -> &str {
        self.0.provider_name()
    }

    async fn stream(
        &self,
        request: ModelCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>, AppError> {
        let scripted = self.0.requests().is_empty();
        let stream = self.0.stream(request).await?;
        if scripted {
            return Ok(stream);
        }
        Ok(Box::pin(futures::stream::pending()))
    }
}

#[tokio::test]
async fn finished_steps_survive_a_turn_that_is_killed_mid_flight() {
    let script = MockAdapter::from_turns(vec![vec![
        ModelStreamEvent::ReasoningBlock(Reasoning::new_with_signature("plan", Some("sig-1".into()))),
        ModelStreamEvent::ToolCall(ToolCall::new(
            "call-1",
            ToolFunction::new("missing", serde_json::json!({})),
        )),
    ]]);
    let (runner, temp, _, _) = test_agent_env(Arc::new(MockAdapter::default()));
    let session_id = runner.session_file().session_id.clone();
    let mut runner = runner.prompt("do it");
    runner.model = Arc::new(HangsAfterScript(script.clone()));

    let mut stream = runner.stream();
    let drain = tokio::spawn(async move {
        while let Some(item) = stream.next().await {
            if let Ok(MultiTurnStreamItem::DisplayBarrier(drained)) = item {
                drained.notify_one();
            }
        }
    });
    crate::mcp::test_server::eventually("the hanging second model call", || script.requests().len() == 2).await;
    drain.abort();
    let _ = drain.await;

    let reopened = rho_harness_core::session::SessionManager::new(temp.path(), Some(&session_id)).unwrap();
    let checkpoint = reopened
        .load_checkpoint()
        .await
        .unwrap()
        .expect("finished step was persisted");
    assert_eq!(checkpoint.first(), Some(&ChatMessage::user("do it")));
    assert!(has_signed_reasoning(&checkpoint));
    assert!(matches!(checkpoint.last(), Some(ChatMessage::User { content })
        if content.iter().any(|c| matches!(c, rho_harness_core::model::UserContent::ToolResult(_)))));
    assert!(reopened.load_messages().await.unwrap().is_empty());
    let kinds: Vec<_> = reopened
        .load_events()
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.kind)
        .collect();
    use rho_harness_core::session::SessionEventKind::{ToolCall as CallEvent, ToolResult as ResultEvent};
    assert_eq!(
        kinds,
        [CallEvent, ResultEvent],
        "finished tools are audited before the turn ends"
    );
}

#[tokio::test]
async fn completing_after_checkpointed_steps_promotes_them_once() {
    let model = tool_turn_then(vec![vec![Ok(ModelStreamEvent::Text("done".into()))]]);

    let (runner, result, _temp) = run_prompt(model).await;

    result.unwrap();
    assert!(runner.session_file().load_checkpoint().await.unwrap().is_none());
    let history = runner.session_file().active_messages().await.unwrap();
    assert_eq!(history.len(), 4, "{history:?}");
    assert_eq!(history.first(), Some(&ChatMessage::user("do it")));
    assert_eq!(history.last(), Some(&ChatMessage::assistant("done")));
    let tool_calls = runner
        .session_file()
        .load_events()
        .await
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == rho_harness_core::session::SessionEventKind::ToolCall)
        .count();
    assert_eq!(tool_calls, 1, "each tool is audited once");
}

#[tokio::test]
async fn a_turn_on_a_memory_store_keeps_its_conversation_there_and_owns_its_own_background_work() {
    let _queue = BACKGROUND_QUEUE_TEST_LOCK.lock().await;
    let model = tool_turn_then(vec![vec![Ok(ModelStreamEvent::Text("done".into()))]]);
    let (runner, _temp, _, _) = test_agent_env(model.clone());
    let session_id = runner.session_file().session_id.clone();
    let store = crate::engine::runner::turn::MemoryStore::new("child-task-9", vec![ChatMessage::user("earlier")]);
    let jobs = crate::tools::jobs::JobManager::global();
    jobs.register(&session_id, "memstore-parent-job".into(), "sleep".into());
    let runner = runner.store(Arc::new(store.clone())).prompt("do it");

    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        run_agent_loop(runner.clone(), &acking_emitter()),
    )
    .await
    .expect("the turn does not wait for the session's own jobs")
    .unwrap();

    jobs.update_status(
        "memstore-parent-job",
        crate::tools::jobs::JobStatus::Failed { error: "over".into() },
    );
    let messages = store.messages();
    assert_eq!(messages.first(), Some(&ChatMessage::user("earlier")));
    assert_eq!(messages.get(1), Some(&ChatMessage::user("do it")));
    assert_eq!(messages.last(), Some(&ChatMessage::assistant("done")));
    assert_eq!(messages.len(), 5, "{messages:?}");
    assert_eq!(
        model.requests()[0].messages.first(),
        Some(&ChatMessage::user("earlier"))
    );
    assert!(runner.session_file().active_messages().await.unwrap().is_empty());
    assert!(runner.session_file().load_checkpoint().await.unwrap().is_none());
}

#[tokio::test]
async fn a_message_waiting_at_the_final_answer_keeps_the_agent_working_except_on_its_last_turn() {
    let _queue = BACKGROUND_QUEUE_TEST_LOCK.lock().await;
    for (budget, expected_requests) in [(5, 2), (1, 1)] {
        let model = Arc::new(MockAdapter::from_turns(vec![
            vec![ModelStreamEvent::Text("first answer".into())],
            vec![ModelStreamEvent::Text("revised answer".into())],
        ]));
        let (runner, _temp, _, _) = test_agent_env(model.clone());
        let owner = runner.session_file().session_id.clone();
        let runner = runner.prompt("review").budget(budget);
        let inbox = runner.inbox.clone().unwrap();
        inbox
            .registry()
            .send(crate::mailbox::Envelope::new(
                crate::mailbox::Party::User,
                owner,
                crate::mailbox::Kind::Command,
                "also check the tests",
            ))
            .unwrap();

        run_agent_loop(runner, &acking_emitter()).await.unwrap();

        let requests = model.requests();
        assert_eq!(requests.len(), expected_requests, "budget {budget}");
        if budget > 1 {
            let sent = serde_json::to_string(&requests[1].messages).unwrap();
            assert!(
                sent.contains("first answer") && sent.contains("also check the tests"),
                "{sent}"
            );
            assert_eq!(inbox.pending(), 0);
        } else {
            assert_eq!(inbox.pending(), 1, "the last turn leaves the message waiting");
        }
    }
}

#[test]
fn retry_delay_honors_provider_waits_up_to_the_cap() {
    use std::time::Duration;
    let limited = |secs: Option<u64>| AppError::RateLimited {
        message: "slow".into(),
        retry_after: secs.map(Duration::from_secs),
    };
    let cases = [
        (1, limited(None), Duration::from_secs(2)),
        (2, limited(None), Duration::from_secs(4)),
        (1, limited(Some(7)), Duration::from_secs(7)),
        (1, limited(Some(0)), Duration::ZERO),
        (1, limited(Some(600)), crate::provider::error::MAX_RETRY_AFTER),
        (1, AppError::Network("reset".into()), Duration::from_secs(2)),
    ];
    for (attempt, error, expected) in cases {
        assert_eq!(retry_delay(attempt, &error), expected, "{error}");
    }
}

#[tokio::test(start_paused = true)]
async fn non_transient_provider_errors_are_not_retried() {
    let model = tool_turn_then(vec![vec![Err(AppError::ContextOverflow("too long".into()))]]);

    let (_runner, result, _temp) = run_prompt(Arc::clone(&model)).await;

    assert!(matches!(result, Err(AppError::ContextOverflow(_))), "{result:?}");
    assert_eq!(model.requests().len(), 2);
}

#[tokio::test(start_paused = true)]
async fn a_dropped_model_request_is_retried_without_failing_the_turn() {
    let model = tool_turn_then(vec![dropped(), vec![Ok(ModelStreamEvent::Text("done".into()))]]);
    let (runner, _temp, _, _) = test_agent_env(Arc::clone(&model));
    let runner = runner.prompt("do it");
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let retries = tokio::spawn(async move {
        let mut retries = Vec::new();
        while let Some(item) = rx.recv().await {
            match item {
                Ok(MultiTurnStreamItem::DisplayBarrier(drained)) => drained.notify_one(),
                Ok(MultiTurnStreamItem::ModelTurnRetried(retry)) => retries.push(retry),
                _ => {}
            }
        }
        retries
    });
    let emitter = TurnEmitter { tx, cancellation: None };

    run_agent_loop(runner.clone(), &emitter).await.unwrap();
    drop(emitter);

    let retries = retries.await.unwrap();
    assert_eq!(retries.len(), 1);
    assert_eq!((retries[0].attempt, retries[0].max_attempts), (1, MODEL_CALL_ATTEMPTS));
    assert!(retries[0].reason.contains("connection reset"));
    assert_eq!(model.requests().len(), 3);
    assert_eq!(model.requests()[1].messages, model.requests()[2].messages);
    let history = runner.session_file().active_messages().await.unwrap();
    assert!(has_signed_reasoning(&history));
    assert_eq!(history.last(), Some(&ChatMessage::assistant("done")));
    assert!(runner.session_file().load_checkpoint().await.unwrap().is_none());
}

#[tokio::test(start_paused = true)]
async fn background_notification_is_injected_once_when_woken() {
    let _queue = BACKGROUND_QUEUE_TEST_LOCK.lock().await;
    let (runner, _temp, mut buf, mut tracker) = test_agent_env(Arc::new(MockAdapter::default()));
    let owner = runner.session_file().session_id.clone();
    let mut ctx = RunnerStepContext::from_test_buffers(&mut buf, &mut tracker);
    let registry = runner.inbox.as_ref().unwrap().registry().clone();
    let enqueue = tokio::spawn(async move {
        tokio::task::yield_now().await;
        registry
            .send(crate::mailbox::Envelope::new(
                crate::mailbox::Party::System,
                owner,
                crate::mailbox::Kind::Event,
                "bg-once-7d1",
            ))
            .unwrap();
    });

    let emitter = acking_emitter();
    let step = wait_step(&runner, &mut ctx, &emitter).await.unwrap();
    enqueue.await.unwrap();
    deliver_messages(&runner, &mut ctx, &emitter).await;

    assert!(matches!(step, WaitStepResult::EventDelivered));
    assert_eq!(
        serde_json::to_string(&*ctx.current_messages)
            .unwrap()
            .matches("bg-once-7d1")
            .count(),
        1
    );
}
