use super::super::{SubagentRuntime, SubagentTool, observed::ASK_PARENT_TOOL};
use super::*;
use crate::provider::adapter::{MockAdapter, ModelStreamEvent};
use crate::tools::engine_tool::{EngineTool, TurnToolContext};
use crate::tools::{GenericTool, ToolResult};
use rho_harness_core::model::{ToolCall, ToolFunction, UserContent};
use rho_harness_core::presentation::SUBAGENT_STARTED_MARKER;
use serde_json::json;
use std::sync::Arc;

fn call(name: &str, args: serde_json::Value) -> Vec<ModelStreamEvent> {
    vec![ModelStreamEvent::ToolCall(ToolCall::new(
        format!("call-{name}"),
        ToolFunction::new(name, args),
    ))]
}

fn text(reply: &str) -> Vec<ModelStreamEvent> {
    vec![ModelStreamEvent::Text(reply.to_string())]
}

fn read_tool() -> Arc<dyn EngineTool> {
    Arc::new(GenericTool::new("read", "reads", json!({}), |_| async {
        Ok(ToolResult::success("fn hot() { loop {} }"))
    }))
}

/// A tool whose rounds finish within the foreground budget, so calls return their results.
fn inline_tool(
    model: Option<crate::engine::compactor::llm::ModelHandle>,
    tools: Vec<Arc<dyn EngineTool>>,
) -> SubagentTool {
    SubagentTool::new(model, tools).with_foreground_budget(std::time::Duration::from_secs(60))
}

fn bound_tool(adapter: &MockAdapter, session: Option<SessionManager>) -> Arc<dyn EngineTool> {
    let ctx = TurnToolContext {
        session,
        ..Default::default()
    };
    inline_tool(Some(Arc::new(adapter.clone())), vec![read_tool()])
        .bind_turn(&ctx)
        .unwrap()
}

fn task_id(content: &str) -> String {
    let after = content.rsplit_once(" \u{b7} task ").unwrap().1;
    after.split(' ').next().unwrap().to_string()
}

fn tool_results(messages: &[ChatMessage]) -> Vec<(String, String)> {
    messages
        .iter()
        .filter_map(|m| match m {
            ChatMessage::User { content } => Some(content),
            _ => None,
        })
        .flatten()
        .filter_map(|c| match c {
            UserContent::ToolResult(r) => Some((r.name.clone(), format!("{:?}", r.content))),
            _ => None,
        })
        .collect()
}

async fn ask(tool: &Arc<dyn EngineTool>, args: serde_json::Value) -> String {
    tool.execute(args).await.unwrap().content
}

/// The subagent tool sharing `runtime`, run for `session` with a 60 second foreground budget.
fn tool_with_runtime(
    adapter: &MockAdapter,
    session: &SessionManager,
    runtime: &SubagentRuntime,
) -> Arc<dyn EngineTool> {
    let ctx = TurnToolContext {
        session: Some(session.clone()),
        ..Default::default()
    };
    inline_tool(Some(Arc::new(adapter.clone())), vec![read_tool()])
        .with_runtime(runtime)
        .bind_turn(&ctx)
        .unwrap()
}

fn ask_turns() -> MockAdapter {
    MockAdapter::from_turns([
        call("read", json!({ "path": "src/hot.rs" })),
        call(
            ASK_PARENT_TOOL,
            json!({ "question": "Optimize for latency or memory?" }),
        ),
        text("Latency: hoisted the allocation out of hot()."),
    ])
}

/// Starts a task whose subagent asks a question, returning its id and the question as the
/// delegating agent receives it.
async fn start_asking(tool: &Arc<dyn EngineTool>, parent: &crate::mailbox::Mailbox) -> (String, String) {
    let started = ask(tool, json!({ "name": "performanceAudit", "prompt": "Audit hot.rs" })).await;
    assert!(
        started.starts_with(SUBAGENT_STARTED_MARKER),
        "a question moves the round to the background at once: {started}"
    );
    let id = started[SUBAGENT_STARTED_MARKER.len()..]
        .split('\'')
        .next()
        .unwrap()
        .to_string();
    parent.wait().await;
    let question = parent.for_model(&parent.take()).concat();
    (id, question)
}

async fn next_notice(parent: &crate::mailbox::Mailbox) -> String {
    parent.wait().await;
    parent.for_model(&parent.take()).concat()
}

#[tokio::test]
async fn a_question_is_answered_while_the_subagent_waits_with_its_work_in_progress() {
    let dir = tempfile::tempdir().unwrap();
    let session = SessionManager::new(dir.path(), None).unwrap();
    let runtime = SubagentRuntime::default();
    let parent = runtime.inbox(&session.session_id.as_str().into());
    let adapter = ask_turns();
    let tool = tool_with_runtime(&adapter, &session, &runtime);

    let (id, question) = start_asking(&tool, &parent).await;
    assert!(id.starts_with("performanceAudit-"), "{id}");
    assert!(
        question.starts_with(&format!(
            "Message from subagent {id} (a reply is expected):\nOptimize for latency or memory?\n\nAnswer with subagent {{\"task_id\": \"{id}\""
        )),
        "{question}"
    );

    let answered = ask(&tool, json!({ "task_id": id, "prompt": "Latency." })).await;

    assert_eq!(
        answered,
        format!("Reply delivered to subagent task '{id}'; it continues from its question.")
    );
    let notice = next_notice(&parent).await;
    assert!(notice.contains("Latency: hoisted the allocation"), "{notice}");
    let last = adapter.requests().pop().unwrap();
    let results = tool_results(&last.messages);
    assert_eq!(results[1].0, ASK_PARENT_TOOL);
    assert!(
        results[1].1.contains("Reply from the delegating agent:\\nLatency."),
        "the reply is the call's result: {results:?}"
    );
    let context = session.subagent_context(&id).await.unwrap();
    let task = context.latest_task().unwrap();
    assert_eq!(context.tasks.len(), 1);
    assert_eq!(task.status.state, TaskState::Completed);
    let roles: Vec<Role> = task.history.iter().map(|m| m.role).collect();
    assert_eq!(roles, [Role::User, Role::Agent], "one round answered the question");
}

#[tokio::test]
async fn a_declined_question_pauses_the_task_and_the_reply_resumes_it() {
    let dir = tempfile::tempdir().unwrap();
    let session = SessionManager::new(dir.path(), None).unwrap();
    let runtime = SubagentRuntime::default();
    let parent = runtime.inbox(&session.session_id.as_str().into());
    let adapter = ask_turns();
    let tool = tool_with_runtime(&adapter, &session, &runtime);
    let (id, _) = start_asking(&tool, &parent).await;

    parent.decline_unanswered();
    let paused = next_notice(&parent).await;
    assert!(
        paused.contains("Question for you: Optimize for latency or memory?"),
        "{paused}"
    );
    assert!(
        paused.contains("\u{b7} input-required: incomplete, 2 turns"),
        "{paused}"
    );

    let done = ask(&tool, json!({ "task_id": id, "prompt": "Latency." })).await;

    assert!(done.starts_with("Latency: hoisted the allocation"), "{done}");
    assert_eq!(task_id(&done), id, "a reply continues the same task");
    assert!(done.contains("\u{b7} completed: 1 turns"), "{done}");
    let last = adapter.requests().pop().unwrap();
    assert_eq!(last.asked(), "Reply from the delegating agent:\nLatency.");
    let results = tool_results(&last.messages);
    assert_eq!(results.len(), 2, "{results:?}");
    assert!(
        results[1].1.contains("Question sent to the delegating agent"),
        "{results:?}"
    );
    assert!(last.tools.iter().any(|t| t.name == ASK_PARENT_TOOL));
    let context = session.subagent_context(&id).await.unwrap();
    let task = context.latest_task().unwrap();
    assert_eq!(context.tasks.len(), 1);
    assert_eq!(task.status.state, TaskState::Completed);
    let roles: Vec<Role> = task.history.iter().map(|m| m.role).collect();
    assert_eq!(roles, [Role::User, Role::Agent, Role::User, Role::Agent]);
    assert_eq!(
        artifact_text(task, RESULT_ARTIFACT),
        Some("Latency: hoisted the allocation out of hot().")
    );
}

#[tokio::test(start_paused = true)]
async fn a_long_round_moves_to_the_background_takes_messages_and_its_task_continues_afterwards() {
    let dir = tempfile::tempdir().unwrap();
    let session = SessionManager::new(dir.path(), None).unwrap();
    let owner = crate::mailbox::AgentId::from(session.session_id.as_str());
    let release = Arc::new(tokio::sync::Notify::new());
    let gate = release.clone();
    let slow_read: Arc<dyn EngineTool> = Arc::new(GenericTool::new("read", "reads", json!({}), move |_| {
        let gate = gate.clone();
        async move {
            gate.notified().await;
            Ok(ToolResult::success("fn hot() {}"))
        }
    }));
    let adapter = MockAdapter::from_turns([
        call("read", json!({ "path": "src/hot.rs" })),
        text("Found one hot loop."),
        text("Second pass found nothing new."),
    ]);
    let background = super::super::background::Backgrounding::new(std::time::Duration::from_secs(60));
    let jobs = background.runtime().jobs().clone();
    let parent_inbox = background.runtime().inbox(&owner);
    let next_notice = || async {
        parent_inbox.wait().await;
        let first = parent_inbox.take().remove(0);
        (first.to, first.body)
    };
    let ctx = TurnToolContext {
        session: Some(session),
        ..Default::default()
    };
    let tool = inline_tool(Some(Arc::new(adapter.clone())), vec![slow_read])
        .with_backgrounding(background)
        .bind_turn(&ctx)
        .unwrap();

    let moved = ask(&tool, json!({ "name": "audit", "prompt": "Audit hot.rs" })).await;
    let id = moved.split('\'').nth(1).unwrap().to_string();
    assert!(moved.contains("Started subagent task"), "{moved}");
    assert!(id.starts_with("audit-"), "{id}");
    let refused = [
        (json!({ "role": "general" }), "runs as 'scout', not 'general'"),
        (json!({ "persona": "skeptic" }), "does not use persona 'skeptic'"),
        (json!({ "model": "default" }), "`model` and `turns` cannot change"),
        (json!({ "turns": 5 }), "`model` and `turns` cannot change"),
    ];
    for (extra, expected) in refused {
        let mut args = json!({ "task_id": id, "prompt": "ignored" });
        args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let result = ask(&tool, args.clone()).await;
        assert!(result.contains(expected), "{args}: {result}");
    }
    let queued = ask(
        &tool,
        json!({ "task_id": id, "role": "scout", "prompt": "Also check cold.rs", "context_slice": "cold.rs is new" }),
    )
    .await;
    assert!(
        queued.starts_with("Message queued for running subagent task"),
        "{queued}"
    );
    let inspected = ask(&tool, json!({ "task_id": id, "inspect": true })).await;
    assert!(inspected.contains("still running in the background"), "{inspected}");

    release.notify_one();
    let (notice_owner, notice) = next_notice().await;
    assert_eq!(
        notice_owner, owner,
        "the notice is for the session that started the task"
    );
    assert!(notice.contains("Found one hot loop."), "{notice}");
    assert!(notice.contains(&format!("task {id} \u{b7} completed")), "{notice}");
    assert!(!notice.contains("Not delivered"), "{notice}");
    let steered = adapter.requests().pop().unwrap();
    let last = serde_json::to_string(steered.messages.last().unwrap()).unwrap();
    assert!(
        last.contains("Message from the delegating agent:\\nContext:\\ncold.rs is new\\n\\nTask:\\nAlso check cold.rs"),
        "{last}"
    );
    tokio::task::yield_now().await;
    assert!(!jobs.has_running(&owner));

    let mut again = ask(&tool, json!({ "task_id": id, "prompt": "again" })).await;
    if again.contains("Started subagent task") {
        again = next_notice().await.1;
    }
    assert!(again.contains("Second pass found nothing new."), "{again}");
}

#[tokio::test]
async fn a_continued_task_keeps_its_persona_and_refuses_a_different_one() {
    let dir = tempfile::tempdir().unwrap();
    let session = SessionManager::new(dir.path(), None).unwrap();
    let adapter = MockAdapter::from_turns([text("Looks fine."), text("Still fine.")]);
    let ctx = TurnToolContext {
        session: Some(session),
        ..Default::default()
    };
    let skeptic = rho_harness_core::agents::PersonaMetadata {
        name: "skeptic".into(),
        description: String::new(),
        location: String::new(),
        instructions: "Demand evidence.".into(),
    };
    let tool = inline_tool(Some(Arc::new(adapter.clone())), vec![read_tool()])
        .with_personas(vec![skeptic])
        .bind_turn(&ctx)
        .unwrap();
    let first = ask(&tool, json!({ "persona": "skeptic", "prompt": "Review" })).await;
    let id = task_id(&first);

    let refused = ask(&tool, json!({ "task_id": id, "persona": "terse", "prompt": "Again" })).await;
    assert!(refused.contains("does not use persona 'terse'"), "{refused}");
    let second = ask(&tool, json!({ "task_id": id, "prompt": "Again" })).await;

    assert!(second.starts_with("Still fine."), "{second}");
    let preamble = adapter.requests().pop().unwrap().preamble.unwrap();
    assert!(preamble.contains("## Persona: skeptic\nDemand evidence."), "{preamble}");
}

#[tokio::test]
async fn a_model_chosen_when_continuing_a_task_is_kept_for_later_continuations() {
    let dir = tempfile::tempdir().unwrap();
    let session = SessionManager::new(dir.path(), None).unwrap();
    let resolver: crate::tools::subagent::SubagentModelResolver = Arc::new(|model: &str| {
        let reply = format!("answered by {model}");
        Ok(Arc::new(MockAdapter::text(&reply)) as crate::provider::adapter::ModelHandle)
    });
    let ctx = TurnToolContext {
        session: Some(session),
        ..Default::default()
    };
    let tool = inline_tool(
        Some(Arc::new(MockAdapter::text("answered by session"))),
        vec![read_tool()],
    )
    .with_model_resolver(resolver)
    .with_model_roles(Some("acme/smol".into()), Some("acme/slow".into()))
    .bind_turn(&ctx)
    .unwrap();

    let first = ask(&tool, json!({ "prompt": "Review" })).await;
    let id = task_id(&first);
    let switched = ask(&tool, json!({ "task_id": id, "model": "slow", "prompt": "Again" })).await;
    let kept = ask(&tool, json!({ "task_id": task_id(&switched), "prompt": "Once more" })).await;

    assert!(first.starts_with("answered by acme/smol"), "{first}");
    assert!(switched.starts_with("answered by acme/slow"), "{switched}");
    assert!(kept.starts_with("answered by acme/slow"), "{kept}");
}

#[tokio::test]
async fn review_feedback_starts_a_new_task_that_keeps_the_subagent_history() {
    let dir = tempfile::tempdir().unwrap();
    let session = SessionManager::new(dir.path(), None).unwrap();
    let adapter = MockAdapter::from_turns([
        call("read", json!({ "path": "src/hot.rs" })),
        text("Found one hot loop."),
    ]);
    let first = ask(
        &bound_tool(&adapter, Some(session.clone())),
        json!({ "name": "perf audit", "prompt": "Audit hot.rs" }),
    )
    .await;
    let first_id = task_id(&first);
    assert!(
        first.contains(&format!("{{\"task_id\": \"{first_id}\", \"prompt\"")),
        "{first}"
    );
    assert!(first.contains("Verify before accepting"), "{first}");
    let session_id = session.session_id.clone();
    drop(session);

    let reopened = SessionManager::new(dir.path(), Some(&session_id)).unwrap();
    adapter.add_turn(
        text("Also found the allocation in cold().")
            .into_iter()
            .map(Ok)
            .collect(),
    );
    let second = ask(
        &bound_tool(&adapter, Some(reopened.clone())),
        json!({ "task_id": first_id, "prompt": "You missed cold(); check it." }),
    )
    .await;

    let second_id = task_id(&second);
    assert_ne!(second_id, first_id, "a finished task is never restarted");
    assert!(second_id.starts_with("perfAudit-"), "{second_id}");
    let request = adapter.requests().pop().unwrap();
    assert_eq!(request.asked(), "You missed cold(); check it.");
    assert_eq!(request.messages[0], ChatMessage::user("Audit hot.rs"));
    assert_eq!(
        request.messages.iter().rev().nth(1),
        Some(&ChatMessage::assistant("Found one hot loop."))
    );
    let context = reopened.subagent_context(&second_id).await.unwrap();
    assert_eq!(context.tasks.len(), 2);
    assert_eq!(context.tasks[1].history[0].reference_task_ids, vec![first_id]);
    assert_eq!(context.transcript.len(), 6);
}

#[tokio::test]
async fn a_turn_limit_waits_for_a_go_ahead_and_continues_with_its_history() {
    let dir = tempfile::tempdir().unwrap();
    let session = SessionManager::new(dir.path(), None).unwrap();
    let adapter = MockAdapter::from_turns([
        call("read", json!({ "path": "a.rs" })),
        text("So far: a.rs is clean."),
        text("Finished: b.rs is clean too."),
    ]);
    let tool = bound_tool(&adapter, Some(session.clone()));

    let stopped = ask(&tool, json!({ "prompt": "Audit a.rs and b.rs", "turns": 1 })).await;
    assert!(
        stopped.starts_with("Incomplete: subagent reached the turn limit of 1"),
        "{stopped}"
    );
    assert!(stopped.contains("The subagent is waiting."), "{stopped}");
    let done = ask(&tool, json!({ "task_id": task_id(&stopped), "prompt": "continue" })).await;

    assert!(done.starts_with("Finished: b.rs is clean too."), "{done}");
    assert_eq!(task_id(&done), task_id(&stopped));
    let request = adapter.requests().pop().unwrap();
    assert_eq!(request.asked(), "continue");
    assert_eq!(
        request.messages.iter().rev().nth(1),
        Some(&ChatMessage::assistant("So far: a.rs is clean."))
    );
}

#[tokio::test]
async fn bad_continuations_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let session = SessionManager::new(dir.path(), None).unwrap();
    let adapter = MockAdapter::from_turns([text("done")]);
    let tool = bound_tool(&adapter, Some(session));
    let id = task_id(&ask(&tool, json!({ "role": "scout", "prompt": "look" })).await);

    let cases = [
        (
            json!({ "task_id": "nope-1234", "prompt": "x" }),
            "unknown subagent task 'nope-1234'",
        ),
        (
            json!({ "task_id": id, "role": "general", "prompt": "x" }),
            "runs as 'scout', not 'general'",
        ),
    ];
    for (args, expected) in cases {
        let result = tool.execute(args).await.unwrap();
        assert!(result.is_error);
        assert!(result.content.contains(expected), "{}", result.content);
    }
    let without_session = ask(&bound_tool(&adapter, None), json!({ "task_id": id, "prompt": "x" })).await;
    assert!(without_session.contains("needs a saved session"), "{without_session}");
}

#[tokio::test]
async fn inspecting_shows_recorded_evidence_without_running_the_subagent() {
    let dir = tempfile::tempdir().unwrap();
    let session = SessionManager::new(dir.path(), None).unwrap();
    let secret = dir.path().join("app.env");
    let adapter = MockAdapter::from_turns([
        vec![
            ModelStreamEvent::ToolCall(ToolCall::new(
                "c1",
                ToolFunction::new("read", json!({ "path": "src/hot.rs" })),
            )),
            ModelStreamEvent::ToolCall(ToolCall::new(
                "c2",
                ToolFunction::new("read", json!({ "path": secret.to_string_lossy() })),
            )),
        ],
        text("hot() allocates in a loop."),
    ]);
    let scope = crate::permission::policy::parse_scope_from_str(
        "[permission.path]\n\"*.env\" = { action = \"deny\", reason = \"secrets\" }\n",
    )
    .unwrap();
    let hook = crate::permission::PermissionHook::with_policy(
        Some(dir.path().to_path_buf()),
        Arc::new(crate::permission::tests::mock::MockHookPresenter::new(false, None)),
        crate::permission::policy::build_policy(None, Some(scope)),
    );
    let ctx = TurnToolContext {
        session: Some(session.clone()),
        permission: Some(Arc::new(hook)),
        ..Default::default()
    };
    let tool = inline_tool(Some(Arc::new(adapter.clone())), vec![read_tool()])
        .bind_turn(&ctx)
        .unwrap();

    let result = ask(&tool, json!({ "name": "audit", "prompt": "Audit hot.rs" })).await;
    let id = task_id(&result);
    assert!(result.contains("Tool outcomes: 1 ok, 1 denied (read "), "{result}");
    assert!(result.contains("1 failed tool call, "), "{result}");
    let requests = adapter.requests().len();

    let inspected = ask(&tool, json!({ "task_id": id, "inspect": true })).await;
    let listed = ask(&tool, json!({ "inspect": true })).await;

    assert_eq!(adapter.requests().len(), requests, "inspecting runs no model");
    for expected in [
        format!("Task `{id}` \u{b7} completed \u{b7} agent scout, name audit (task 1 of 1"),
        "[parent] Audit hot.rs".to_string(),
        "Final answer:\nhot() allocates in a loop.".to_string(),
        "ok      read src/hot.rs".to_string(),
        "denied  read ".to_string(),
        "Files read or edited so far: ".to_string(),
    ] {
        assert!(inspected.contains(&expected), "missing {expected:?} in:\n{inspected}");
    }
    assert!(
        !inspected.contains("[agent] hot() allocates"),
        "a last message that is the final answer is shown once:\n{inspected}"
    );
    let summary = format!(
        "`{id}` \u{b7} completed, 1 failed tool call \u{b7} agent scout, name audit \u{b7} 1 task(s) \u{b7} \"Audit hot.rs\""
    );
    assert!(listed.contains(&summary), "{listed}");
    let unknown = tool
        .execute(json!({ "task_id": "nope", "inspect": true }))
        .await
        .unwrap();
    assert!(unknown.is_error && unknown.content.contains("unknown subagent task 'nope'"));
    let no_session = ask(&bound_tool(&adapter, None), json!({ "inspect": true })).await;
    assert!(no_session.contains("needs a saved session"), "{no_session}");
}

#[tokio::test]
async fn a_continued_subagent_is_told_which_files_changed_since_it_read_them() {
    let dir = tempfile::tempdir().unwrap();
    let session = SessionManager::new(&dir.path().join("sessions"), None).unwrap();
    let watched = dir.path().join("lib.rs");
    let untouched = dir.path().join("other.rs");
    std::fs::write(&watched, "fn a() {}").unwrap();
    std::fs::write(&untouched, "fn b() {}").unwrap();
    let read = |path: &std::path::Path| call("read", json!({ "path": path.to_string_lossy() }));
    let adapter = MockAdapter::from_turns([read(&watched), read(&untouched), text("reviewed")]);
    let tool = bound_tool(&adapter, Some(session));
    let first = ask(&tool, json!({ "prompt": "Review lib.rs" })).await;

    std::fs::write(&watched, "fn a() { changed() }").unwrap();
    adapter.add_turn(vec![Ok(ModelStreamEvent::Text("re-reviewed".into()))]);
    let second = ask(&tool, json!({ "task_id": task_id(&first), "prompt": "Check again" })).await;

    let shown = watched.to_string_lossy().to_string();
    let prompt = adapter.requests().pop().unwrap().asked();
    assert_eq!(
        prompt,
        format!(
            "[These files changed since you last read or edited them; re-read them before relying on what you saw earlier: {shown}]\n\nCheck again"
        )
    );
    let told = format!("Told the subagent these files changed since it last saw them: {shown}\n");
    assert!(second.contains(&told), "{second}");
}

#[tokio::test]
async fn bash_changes_are_attributed_and_later_changes_reach_a_continued_subagent() {
    let _repo = rho_test_support::TestDir::new();
    let root = crate::tools::subagent::test_repo::committed_repo(&_repo, &["other.rs"]);
    let sessions = tempfile::tempdir().unwrap();
    let session = SessionManager::new(sessions.path(), None).unwrap();
    let bash_root = root.clone();
    let bash = GenericTool::new("bash", "runs", json!({}), move |_| {
        let target = bash_root.join("gen.rs");
        async move {
            std::fs::write(target, "generated").unwrap();
            Ok(ToolResult::success("done"))
        }
    });
    let outside_root = root.clone();
    let outside = GenericTool::new(
        "web_fetch",
        "stands in for a change made by another process",
        json!({}),
        move |_| {
            let target = outside_root.join("notes.md");
            async move {
                std::fs::write(target, "edited elsewhere").unwrap();
                Ok(ToolResult::success("ok"))
            }
        },
    );
    let adapter = MockAdapter::from_turns([
        vec![
            ModelStreamEvent::ToolCall(ToolCall::new(
                "b",
                ToolFunction::new("bash", json!({ "command": "make gen" })),
            )),
            ModelStreamEvent::ToolCall(ToolCall::new(
                "w",
                ToolFunction::new("web_fetch", json!({ "url": "x" })),
            )),
        ],
        text("generated"),
    ]);
    let ctx = TurnToolContext {
        session: Some(session),
        project: Some(Arc::new(crate::engine::context::ProjectContext::for_tests(&root))),
        ..Default::default()
    };
    let tool = inline_tool(Some(Arc::new(adapter.clone())), vec![Arc::new(bash), Arc::new(outside)])
        .bind_turn(&ctx)
        .unwrap();

    let first = ask(&tool, json!({ "role": "general", "prompt": "generate" })).await;

    assert!(
        first.contains("Changed while its commands ran: gen.rs (bash: make gen)\n"),
        "{first}"
    );
    assert!(
        first.contains("not by its tools (editors, other agents, or processes): notes.md\n"),
        "{first}"
    );
}

struct WorktreeSetup {
    dir: tempfile::TempDir,
    root: std::path::PathBuf,
    store: crate::worktree::WorktreeStore,
    adapter: MockAdapter,
    tool: Arc<dyn EngineTool>,
}

fn write_turn(path: &str, body: &str) -> Vec<ModelStreamEvent> {
    call("write", json!({ "path": path, "content": body }))
}

/// A repository and a subagent tool whose `write` runs for real inside each subagent's worktree.
fn worktree_setup(turns: Vec<Vec<ModelStreamEvent>>) -> WorktreeSetup {
    let (dir, root) = crate::worktree::git::test_repo::create(&[("a.rs", "base\n"), ("other.rs", "v1\n")]);
    let store = crate::worktree::WorktreeStore::new(dir.path().join("store"));
    let session = SessionManager::new(&dir.path().join("sessions"), None).unwrap();
    let placeholder: Arc<dyn EngineTool> = Arc::new(GenericTool::new(
        "write",
        "replaced in the worktree",
        json!({}),
        |_| async { Ok(ToolResult::error("the placeholder must never run")) },
    ));
    let adapter = MockAdapter::from_turns(turns);
    let ctx = TurnToolContext {
        session: Some(session),
        project: Some(Arc::new(crate::engine::context::ProjectContext::for_tests(&root))),
        ..Default::default()
    };
    let tool = inline_tool(Some(Arc::new(adapter.clone())), vec![placeholder])
        .with_worktrees(store.clone())
        .bind_turn(&ctx)
        .unwrap();
    WorktreeSetup {
        dir,
        root,
        store,
        adapter,
        tool,
    }
}

#[tokio::test]
async fn inspecting_an_unmerged_worktree_task_checks_files_in_its_worktree() {
    let s = worktree_setup(vec![write_turn("a.rs", "agent\n"), text("wrote a.rs")]);
    let first = ask(
        &s.tool,
        json!({ "name": "fixer", "role": "general", "prompt": "fix a.rs" }),
    )
    .await;

    let inspected = ask(&s.tool, json!({ "task_id": task_id(&first), "inspect": true })).await;

    assert!(inspected.contains("Files read or edited so far: a.rs"), "{inspected}");
    assert!(
        !inspected.contains("Changed since the agent last saw them"),
        "{inspected}"
    );
}

#[tokio::test]
async fn subagents_work_in_their_own_worktree_and_feedback_continues_there() {
    let s = worktree_setup(vec![write_turn("a.rs", "agent\n"), text("wrote a.rs")]);

    let first = ask(
        &s.tool,
        json!({ "name": "fixer", "role": "general", "prompt": "fix a.rs" }),
    )
    .await;
    let worktree = s.store.list(&s.root).await.pop().unwrap();

    assert!(
        first.contains(&format!("These changes are in worktree `{}`", worktree.id)),
        "{first}"
    );
    assert_eq!(
        std::fs::read_to_string(s.root.join("a.rs")).unwrap(),
        "base\n",
        "main is untouched"
    );
    assert_eq!(std::fs::read_to_string(worktree.path.join("a.rs")).unwrap(), "agent\n");

    s.adapter
        .add_turn(write_turn("b.rs", "more\n").into_iter().map(Ok).collect());
    s.adapter
        .add_turn(vec![Ok(ModelStreamEvent::Text("added b.rs".into()))]);
    let second = ask(
        &s.tool,
        json!({ "task_id": task_id(&first), "prompt": "also add b.rs" }),
    )
    .await;

    assert_eq!(
        s.store.list(&s.root).await,
        std::slice::from_ref(&worktree),
        "feedback continues in the same worktree"
    );
    assert!(
        second.contains("not in your working tree: a.rs, b.rs (added)."),
        "{second}"
    );
    assert!(second.contains("Files changed: b.rs\nTool outcomes"), "{second}");
    assert!(
        !s.adapter
            .requests()
            .pop()
            .unwrap()
            .asked()
            .contains("previous worktree")
    );
}

#[tokio::test]
async fn after_a_merge_a_continued_subagent_gets_a_fresh_worktree_and_hears_what_changed() {
    let s = worktree_setup(vec![write_turn("a.rs", "agent\n"), text("wrote a.rs")]);
    let first = ask(&s.tool, json!({ "role": "general", "prompt": "fix a.rs" })).await;
    let worktree = s.store.list(&s.root).await.pop().unwrap();
    std::fs::write(s.root.join("other.rs"), "parent edit\n").unwrap();
    let parent = crate::tools::WorktreeTool::new(&s.root, s.store.clone());

    let merged = parent
        .execute(json!({ "action": "merge", "id": worktree.id }))
        .await
        .unwrap();
    s.adapter
        .add_turn(vec![Ok(ModelStreamEvent::Text("looked again".into()))]);
    let again = ask(
        &s.tool,
        json!({ "task_id": task_id(&first), "prompt": "check other.rs" }),
    )
    .await;

    assert!(
        merged.content.starts_with("Merged 1 file(s): a.rs."),
        "{}",
        merged.content
    );
    assert_eq!(std::fs::read_to_string(s.root.join("a.rs")).unwrap(), "agent\n");
    let prompt = s.adapter.requests().pop().unwrap().asked();
    assert!(prompt.starts_with("[Your previous worktree "), "{prompt}");
    assert!(
        prompt.ends_with("[Other files changed in the workspace since your last turn: other.rs.]\n\ncheck other.rs"),
        "{prompt}"
    );
    assert!(
        !again.contains("These changes are in worktree"),
        "no changes, no worktree: {again}"
    );
    assert!(s.store.list(&s.root).await.is_empty());
    drop(s.dir);
}

#[test]
fn only_subagents_limited_to_read_only_tools_run_in_parallel() {
    let adapter = MockAdapter::from_turns([]);
    let write: Arc<dyn EngineTool> = Arc::new(GenericTool::new("write", "writes", json!({}), |_| async {
        Ok(ToolResult::success("written"))
    }));
    let tool = inline_tool(Some(Arc::new(adapter)), vec![read_tool(), write]);
    let cases = [
        (json!({ "role": "scout", "prompt": "x" }), true),
        (json!({ "role": "general", "prompt": "x" }), false),
        (json!({ "task_id": "t-1", "prompt": "x" }), false),
        (json!({ "inspect": true }), true),
    ];
    for (args, parallel) in cases {
        assert_eq!(tool.concurrency_safe(&args), parallel, "{args}");
    }
}

#[tokio::test]
async fn a_compacted_round_replaces_the_saved_conversation_and_the_next_round_continues_from_it() {
    let dir = tempfile::tempdir().unwrap();
    let session = SessionManager::new(dir.path(), None).unwrap();
    let adapter = MockAdapter::from_result_turns([
        call("read", json!({ "path": "a.rs" })).into_iter().map(Ok).collect(),
        call("read", json!({ "path": "b.rs" })).into_iter().map(Ok).collect(),
        vec![Ok(ModelStreamEvent::Text("first done".into()))],
        vec![Err(rho_harness_core::error::AppError::ContextOverflow(
            "too long".into(),
        ))],
        vec![Ok(ModelStreamEvent::Text("Summary: read a.rs and b.rs.".into()))],
        vec![Ok(ModelStreamEvent::Text("second done".into()))],
        vec![Ok(ModelStreamEvent::Text("third done".into()))],
    ]);
    let ctx = TurnToolContext {
        session: Some(session.clone()),
        ..Default::default()
    };
    let tool = inline_tool(Some(Arc::new(adapter.clone())), vec![read_tool()]);
    tool.models()
        .set_context_policy(crate::tools::subagent::tests::window(1_000_000, 1));
    let tool = tool.bind_turn(&ctx).unwrap();

    let first = ask(&tool, json!({ "prompt": "Audit" })).await;
    let second = ask(&tool, json!({ "task_id": task_id(&first), "prompt": "Go on" })).await;
    assert!(second.starts_with("second done"), "{second}");

    let saved = session.subagent_context(&task_id(&second)).await.unwrap().transcript;
    assert!(
        matches!(&saved[0], ChatMessage::System { content } if content.contains("Summary")),
        "{saved:?}"
    );
    assert_eq!(
        saved
            .iter()
            .filter(|m| **m == ChatMessage::assistant("first done"))
            .count(),
        1,
        "kept once as recent context, never duplicated"
    );
    assert!(saved.len() < 6, "older turns were folded: {saved:?}");
    assert_eq!(saved.iter().filter(|m| **m == ChatMessage::user("Go on")).count(), 1);
    assert_eq!(saved.last(), Some(&ChatMessage::assistant("second done")));

    let third = ask(&tool, json!({ "task_id": task_id(&second), "prompt": "Once more" })).await;
    assert!(third.starts_with("third done"), "{third}");
    let request = adapter.requests().pop().unwrap();
    assert!(
        matches!(&request.messages[0], ChatMessage::System { .. }),
        "continues from the compacted conversation"
    );
    assert_eq!(request.asked(), "Once more");
}

#[tokio::test]
async fn subagents_without_a_parent_session_get_no_ask_parent_tool() {
    let adapter = MockAdapter::from_turns([text("done")]);
    let result = ask(&bound_tool(&adapter, None), json!({ "prompt": "look" })).await;

    assert!(!result.contains("task "), "{result}");
    assert!(!adapter.requests()[0].tools.iter().any(|t| t.name == ASK_PARENT_TOOL));
}
