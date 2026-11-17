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
    let runner = AgentRunner::new(model, Vec::new(), sm, sink);
    (
        runner,
        temp,
        TestStepBuffers::default(),
        RepeatedCallTracker::new(std::path::PathBuf::from(".")),
    )
}

static BACKGROUND_QUEUE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
    let mut ctx = RunnerStepContext::from_test_buffers(&mut buf, &mut tracker);
    let emitter = acking_emitter();

    assert!(!wait_for_background_jobs(&runner, &mut ctx, &emitter).await.unwrap());
    crate::tools::jobs::JobManager::global().register("bg-1".into(), "echo 1".into());
    tokio::spawn(async {
        crate::tools::jobs::JobManager::global().update_status(
            "bg-1",
            crate::tools::jobs::JobStatus::Completed {
                exit_code: 0,
                output: "ok".into(),
            },
        );
        crate::engine::runner::turn::types::enqueue_background_notification(
            "<system-notice>done</system-notice>".into(),
        );
    });
    assert!(wait_for_background_jobs(&runner, &mut ctx, &emitter).await.unwrap());
    crate::tools::jobs::JobManager::global().register("bg-2".into(), "sleep".into());
    sig.cancel();
    assert!(matches!(
        wait_for_background_jobs(&runner, &mut ctx, &emitter).await,
        Err(AppError::Cancelled(_))
    ));
    crate::tools::jobs::JobManager::global()
        .update_status("bg-2", crate::tools::jobs::JobStatus::Failed { error: "a".into() });
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
    };

    let outcomes = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        execute_concurrent_tool_calls(&calls, &mut ctx),
    )
    .await
    .expect("calls must overlap; a sequential run would deadlock on the barrier");

    let texts: Vec<String> = outcomes.iter().map(|(m, _)| result_text(m)).collect();
    assert_eq!(texts, vec!["alpha done", "beta done"]);
    assert!(outcomes.iter().all(|(_, stop)| stop.is_none()));
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
    assert!(runner.session_manager.active_messages().await.unwrap().is_empty());
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
        .session_manager
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
    let history = runner.session_manager.active_messages().await.unwrap();
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
    let checkpoint = runner.session_manager.load_checkpoint().await.unwrap().unwrap();
    assert_eq!(checkpoint.len(), 4);
    assert_eq!(checkpoint.last(), Some(&ChatMessage::user(EMPTY_COMPLETION_NUDGE)));
    runner
        .session_manager
        .promote_checkpoint(vec![ChatMessage::user("next"), ChatMessage::assistant("done")])
        .await
        .unwrap();
}

#[tokio::test(start_paused = true)]
async fn background_notification_is_injected_once_when_woken() {
    let _queue = BACKGROUND_QUEUE_TEST_LOCK.lock().await;
    let (runner, _temp, mut buf, mut tracker) = test_agent_env(Arc::new(MockAdapter::default()));
    let mut ctx = RunnerStepContext::from_test_buffers(&mut buf, &mut tracker);
    let mut notif_rx = crate::engine::runner::turn::types::subscribe_background_notifications();
    let enqueue = tokio::spawn(async {
        tokio::task::yield_now().await;
        crate::engine::runner::turn::types::enqueue_background_notification("bg-once-7d1".to_string());
    });

    let step = wait_step(&runner, &mut notif_rx, &mut ctx).await.unwrap();
    enqueue.await.unwrap();
    drain_background_to_context(&mut ctx);

    assert!(matches!(step, WaitStepResult::EventDelivered));
    assert_eq!(
        serde_json::to_string(&*ctx.current_messages)
            .unwrap()
            .matches("bg-once-7d1")
            .count(),
        1
    );
}
