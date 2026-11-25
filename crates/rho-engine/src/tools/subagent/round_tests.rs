//! Rounds of the subagent turn loop: compaction, failures, the agent view, evidence in results,
//! and the worktree language servers a round starts.

use super::tests::{CollectedProgress, TestModel, counting_tool, named_tool, text_turn, tool_call_turn};
use super::*;
use crate::engine::runner::turn::types::CancellationSignal;
use crate::provider::adapter::{MockAdapter, ModelAdapter, ModelCompletionRequest, ModelStreamEvent};
use crate::test_support::TestDir;
use crate::tools::{GenericTool, ToolResult};
use rho_harness_core::model::{ChatMessage, ToolCall, ToolFunction};
use rho_harness_core::presentation::ToolStreamPort;
use serde_json::json;
use std::sync::atomic::AtomicUsize;

#[tokio::test]
async fn a_subagent_that_overflows_its_context_compacts_and_finishes_the_round() {
    let adapter = MockAdapter::from_result_turns([
        tool_call_turn(&[("read", "big")]).into_iter().map(Ok).collect(),
        tool_call_turn(&[("read", "bigger")]).into_iter().map(Ok).collect(),
        vec![Err(AppError::ContextOverflow("prompt is too long".into()))],
        vec![Ok(ModelStreamEvent::Text(
            "Read big and bigger; both were fine.".into(),
        ))],
        vec![Ok(ModelStreamEvent::Text("Done after compacting.".into()))],
    ]);
    let tools = vec![counting_tool("read", Arc::new(AtomicUsize::new(0)), "x".repeat(4_000))];
    let runner =
        SubagentRunner::new(Arc::new(adapter.clone()), SubagentRole::Scout, &tools, 5).with_keep_recent_tokens(1);

    let round = runner.run_round(Vec::new(), round::RoundInput::Prompt("look")).await;

    assert_eq!(round.end, round::RoundEnd::Answered("Done after compacting.".into()));
    assert_eq!(round.turns, 3, "turns before and after the compaction add up");
    assert!(
        matches!(&round.transcript[0], ChatMessage::System { content } if content.contains("Read big and bigger")),
        "{:?}",
        round.transcript[0]
    );
    let last = adapter.requests().pop().unwrap();
    assert!(matches!(&last.messages[0], ChatMessage::System { .. }));
    assert!(
        last.messages.len() < 6,
        "older turns were folded: {}",
        last.messages.len()
    );
}

#[tokio::test]
async fn a_failure_before_the_first_reply_keeps_the_instruction_in_the_transcript() {
    let adapter = MockAdapter::from_result_turns([vec![Err(AppError::Other(anyhow::anyhow!("boom")))]]);
    let runner = SubagentRunner::new(Arc::new(adapter), SubagentRole::Scout, &[], 5);

    let round = runner
        .run_round(
            vec![ChatMessage::user("earlier")],
            round::RoundInput::Prompt("check the lexer"),
        )
        .await;

    assert!(matches!(round.end, round::RoundEnd::Failed(_)), "{:?}", round.end);
    assert_eq!(round.transcript.last(), Some(&ChatMessage::user("check the lexer")));
    assert_eq!(round.transcript.len(), 2);
}

#[tokio::test]
async fn an_overflow_on_the_first_call_compacts_and_still_gives_the_model_the_instruction() {
    let adapter = MockAdapter::from_result_turns([
        vec![Err(AppError::ContextOverflow("prompt is too long".into()))],
        vec![Ok(ModelStreamEvent::Text("Earlier: the parser was reviewed.".into()))],
        vec![Ok(ModelStreamEvent::Text("Lexer checked.".into()))],
    ]);
    let mut history: Vec<ChatMessage> = (0..6)
        .flat_map(|i| {
            [
                ChatMessage::user(format!("step {i}")),
                ChatMessage::assistant("x".repeat(2_000)),
            ]
        })
        .collect();
    history.push(ChatMessage::user("the plan is settled"));
    let runner = SubagentRunner::new(Arc::new(adapter.clone()), SubagentRole::Scout, &[], 5).with_keep_recent_tokens(1);

    let round = runner
        .run_round(history, round::RoundInput::Prompt("check the lexer"))
        .await;

    assert_eq!(round.end, round::RoundEnd::Answered("Lexer checked.".into()));
    assert!(round.compacted);
    let last = adapter.requests().pop().unwrap();
    assert_eq!(last.asked(), "check the lexer");
    assert!(matches!(&last.messages[0], ChatMessage::System { .. }));
    assert_eq!(
        round
            .transcript
            .iter()
            .filter(|m| **m == ChatMessage::user("check the lexer"))
            .count(),
        1,
        "the instruction is in the conversation once"
    );
}

#[tokio::test(start_paused = true)]
async fn a_rounds_own_background_jobs_stop_when_the_round_is_stopped() {
    let adapter = MockAdapter::from_turns([text_turn("done")]);
    let boxes = crate::mailbox::Mailboxes::default();
    let inbox = boxes.open("round-jobs-7a1", None);
    let job = crate::tools::jobs::JobManager::global().register(
        "round-jobs-7a1",
        "round-jobs-7a1-job".into(),
        "sleep".into(),
    );
    let stop = CancellationSignal::default();
    let runner = SubagentRunner::new(Arc::new(adapter), SubagentRole::Scout, &[], 5)
        .with_inbox(inbox)
        .with_cancellation(stop.clone());

    let round = tokio::spawn(async move { runner.run_round(Vec::new(), round::RoundInput::Prompt("look")).await });
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    assert!(!round.is_finished(), "the round waits for its own background job");
    stop.cancel();
    let round = tokio::time::timeout(std::time::Duration::from_secs(5), round)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(round.end, round::RoundEnd::Cancelled);
    assert!(job.is_cancelled(), "the round's own job is stopped with it");
    crate::tools::jobs::JobManager::global().update_status(
        "round-jobs-7a1-job",
        crate::tools::jobs::JobStatus::Failed {
            error: "test over".into(),
        },
    );
}

#[derive(Debug)]
struct CrashingModel;

#[async_trait::async_trait]
impl ModelAdapter for CrashingModel {
    fn model_name(&self) -> &str {
        "crash"
    }
    fn provider_name(&self) -> &str {
        "test"
    }
    async fn stream(
        &self,
        _request: ModelCompletionRequest,
    ) -> Result<crate::provider::adapter::ModelEventStream, AppError> {
        panic!("the provider client crashed");
    }
}

#[tokio::test]
async fn a_turn_that_dies_is_reported_as_a_failure_not_an_interruption() {
    let runner = SubagentRunner::new(Arc::new(CrashingModel), SubagentRole::Scout, &[], 5);

    let round = runner.run_round(Vec::new(), round::RoundInput::Prompt("go")).await;

    assert!(
        matches!(&round.end, round::RoundEnd::Failed(text) if text.contains("ended unexpectedly")),
        "{:?}",
        round.end
    );
}

#[tokio::test]
async fn the_agent_view_gets_the_reply_and_reasoning_from_the_model_stream() {
    let adapter = MockAdapter::from_turns([vec![
        ModelStreamEvent::Reasoning("check the file first. ".into()),
        ModelStreamEvent::Reasoning("then answer".into()),
        ModelStreamEvent::Text("The file ".into()),
        ModelStreamEvent::Text("is fine.".into()),
        ModelStreamEvent::Finish(crate::provider::adapter::FinishReason::Stop),
    ]]);
    let monitor = rho_harness_core::agents::AgentMonitor::default();
    let ctx = TurnToolContext {
        monitor: Some(monitor.clone()),
        ..Default::default()
    };
    let tool = SubagentTool::new(Some(Arc::new(adapter)), vec![named_tool("read")])
        .bind_turn(&ctx)
        .unwrap();

    let result = tool
        .execute(json!({ "role": "scout", "prompt": "look" }))
        .await
        .unwrap();

    assert!(result.content.starts_with("The file is fine."), "{}", result.content);
    let record = monitor.record(monitor.summaries()[0].id).unwrap();
    assert_eq!(
        record.events,
        [
            rho_harness_core::agents::AgentEvent::Task("look".into()),
            rho_harness_core::agents::AgentEvent::Thinking("check the file first. then answer".into()),
            rho_harness_core::agents::AgentEvent::Text("The file is fine.".into()),
        ]
    );
}

#[tokio::test]
async fn named_subagents_label_progress_the_monitor_and_the_result() {
    let adapter = MockAdapter::from_turns([tool_call_turn(&[("read", "x")]), text_turn("done")]);
    let progress = Arc::new(CollectedProgress::default());
    let monitor = rho_harness_core::agents::AgentMonitor::default();
    let ctx = TurnToolContext {
        progress: ToolStreamPort::new(Some(progress.clone())),
        monitor: Some(monitor.clone()),
        ..Default::default()
    };
    let tool = SubagentTool::new(Some(Arc::new(adapter)), vec![named_tool("read")])
        .bind_turn(&ctx)
        .unwrap();

    let result = tool
        .execute(json!({ "name": "performance audit", "role": "scout", "prompt": "go" }))
        .await
        .unwrap();

    assert_eq!(
        *progress.0.lock().unwrap(),
        vec!["subagent performanceAudit turn 1/100: read\n"]
    );
    let summaries = monitor.summaries();
    assert_eq!(summaries[0].label, "performanceAudit · scout");
    assert_eq!(summaries[0].status, rho_harness_core::agents::AgentStatus::Done);
    assert!(
        result
            .content
            .ends_with("[subagent performanceAudit: 2 turns, 1 tool calls, 0 tokens, mock-provider/mock-model]"),
        "{}",
        result.content
    );
}

fn edit_tool(fails_for: &'static str) -> Arc<dyn EngineTool> {
    Arc::new(GenericTool::new("edit", "edits", json!({}), move |args| async move {
        if args["path"] == fails_for {
            return Ok(ToolResult::error("oldText not found"));
        }
        Ok(ToolResult::success("edited"))
    }))
}

fn edit_turn(paths: &[&str]) -> Vec<ModelStreamEvent> {
    paths
        .iter()
        .enumerate()
        .map(|(i, path)| {
            ModelStreamEvent::ToolCall(ToolCall::new(
                format!("call-{i}"),
                ToolFunction::new("edit", json!({ "path": path })),
            ))
        })
        .collect()
}

#[tokio::test]
async fn results_list_changed_files_and_failed_tool_calls() {
    let adapter = MockAdapter::from_turns([
        edit_turn(&["src/a.rs", "./src/../src/missing.rs"]),
        text_turn("fixed it"),
    ]);
    let runner = SubagentRunner::new(
        Arc::new(adapter),
        SubagentRole::General,
        &[edit_tool("./src/../src/missing.rs")],
        5,
    );

    let result = runner.run("fix", None).await;

    assert_eq!(
        result.content,
        "fixed it\n\nFiles changed: src/a.rs\nTool outcomes: 1 ok, 1 error (edit src/missing.rs: oldText not found)\n\n[subagent: 1 failed tool call, 2 turns, 2 tool calls, 0 tokens, mock-provider/mock-model]"
    );
}

#[tokio::test]
async fn an_empty_final_answer_is_salvaged_instead_of_discarding_the_work() {
    let adapter = MockAdapter::from_turns([
        edit_turn(&["src/a.rs"]),
        text_turn(""),
        text_turn("Summary: edited src/a.rs to fix the parser."),
    ]);
    let monitor = rho_harness_core::agents::AgentMonitor::default();
    let runner = SubagentRunner::new(Arc::new(adapter.clone()), SubagentRole::General, &[edit_tool("")], 5)
        .with_context(TurnToolContext {
            monitor: Some(monitor.clone()),
            ..Default::default()
        });

    let result = runner.run("fix the parser", None).await;

    assert!(!result.is_error, "{}", result.content);
    assert!(
        result
            .content
            .starts_with("Incomplete: subagent ended without writing a final answer before finishing")
    );
    assert!(result.content.contains("Summary: edited src/a.rs to fix the parser."));
    assert!(result.content.contains("Files changed: src/a.rs"));
    assert!(result.content.contains("incomplete, 2 turns, 1 tool calls"));
    assert_eq!(adapter.requests().len(), 3, "one extra call to summarize");
    assert_eq!(
        monitor.summaries()[0].status,
        rho_harness_core::agents::AgentStatus::InputRequired
    );
}

fn lsp_worktree() -> TestDir {
    let worktree = TestDir::new();
    std::fs::write(worktree.join("Cargo.toml"), "").unwrap();
    std::fs::create_dir(worktree.join("src")).unwrap();
    std::fs::write(worktree.join("src/lib.rs"), "fn helper() {}\n").unwrap();
    worktree
}

#[tokio::test(start_paused = true)]
async fn finished_worktree_round_shuts_down_the_worktree_language_servers() {
    use crate::lsp::manager::{is_registered, register_manager};
    use crate::lsp::test_server::{FakeOptions, fake_manager};
    let worktree = lsp_worktree();
    let (manager, launcher) = fake_manager(&worktree, FakeOptions::default());
    let opened: lsp::WorktreeLsp = {
        let manager = manager.clone();
        Arc::new(move |_| {
            register_manager(&manager);
            manager.clone()
        })
    };
    let parent_lsp: Arc<dyn EngineTool> = Arc::new(crate::tools::lsp::LspTool::new(&worktree, manager.clone()));
    let runner = SubagentRunner::new(
        Arc::new(TestModel::answering("done")),
        SubagentRole::Scout,
        &[parent_lsp],
        5,
    )
    .with_worktree_lsp(Some(opened))
    .in_worktree(&worktree);
    let lsp = runner.tool("lsp").unwrap();
    lsp.execute(json!({"action": "symbols", "path": "src/lib.rs"}))
        .await
        .unwrap();
    assert_eq!(manager.running_clients().len(), 1);
    assert!(is_registered(&worktree));

    runner.worktree_lsp_guard().close().await;

    assert!(manager.running_clients().is_empty());
    assert!(launcher.received.lock().unwrap().iter().any(|m| m == "shutdown"));
    assert!(!is_registered(&worktree));
}

#[tokio::test(start_paused = true)]
async fn dropping_the_guard_of_an_aborted_round_still_shuts_the_worktree_language_servers_down() {
    use crate::lsp::manager::{is_registered, register_manager};
    use crate::lsp::test_server::{FakeOptions, fake_manager};
    let worktree = lsp_worktree();
    let (manager, launcher) = fake_manager(&worktree, FakeOptions::default());
    register_manager(&manager);
    manager.client_for(&worktree.join("src/lib.rs")).await.unwrap();
    let opened: lsp::WorktreeLsp = {
        let manager = manager.clone();
        Arc::new(move |_| manager.clone())
    };
    let runner = SubagentRunner::new(Arc::new(TestModel::answering("done")), SubagentRole::Scout, &[], 5)
        .with_worktree_lsp(Some(opened))
        .in_worktree(&worktree);
    assert_eq!(manager.running_clients().len(), 1);

    drop(runner.worktree_lsp_guard());
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    assert!(manager.running_clients().is_empty());
    assert!(launcher.received.lock().unwrap().iter().any(|m| m == "shutdown"));
    assert!(!is_registered(&worktree));
}

#[tokio::test(start_paused = true)]
async fn a_cancelled_round_does_not_wait_for_the_language_servers_to_shut_down() {
    use crate::lsp::manager::{is_registered, register_manager};
    use crate::lsp::test_server::{FakeOptions, fake_manager};
    let worktree = lsp_worktree();
    let (manager, launcher) = fake_manager(&worktree, FakeOptions::default());
    register_manager(&manager);
    manager.client_for(&worktree.join("src/lib.rs")).await.unwrap();
    let opened: lsp::WorktreeLsp = {
        let manager = manager.clone();
        Arc::new(move |_| manager.clone())
    };
    let runner = SubagentRunner::new(Arc::new(TestModel::answering("done")), SubagentRole::Scout, &[], 5)
        .with_worktree_lsp(Some(opened))
        .in_worktree(&worktree);

    runner.worktree_lsp_guard().close_unless_cancelled(true).await;
    assert!(!launcher.received.lock().unwrap().iter().any(|m| m == "shutdown"));

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    assert!(manager.running_clients().is_empty());
    assert!(launcher.received.lock().unwrap().iter().any(|m| m == "shutdown"));
    assert!(!is_registered(&worktree));
}
