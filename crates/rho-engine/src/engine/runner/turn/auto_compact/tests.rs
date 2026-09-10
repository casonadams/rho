//! Tests for the mid-run auto-compaction hook: compaction at the
//! before-next-assistant-call boundary with a history patch, and the
//! ephemeral fallback when the durable compaction cannot persist.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rho_harness_core::config::{Config, PermissionConfig};
use rho_harness_core::presentation::activity::ActivityToken;
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::stream::ToolStreamPort;
use rho_harness_core::presentation::{SessionStatus, ToolLine, WelcomeDisplay};
use rho_harness_core::session::tree::TreeNodeKind;
use rig::agent::hook::CompletionCallAction;
use rig::completion::Usage;
use rig::memory::ConversationMemory;
use rig::message::Message;
use rig::message::{AssistantContent, ToolResultContent, UserContent};
use rig::test_utils::{MockCompletionModel, MockStreamEvent};
use rig::tool::{DynamicTool, ToolOutput};

use crate::engine::eval::mock::{MockEngineConfig, final_event, mock_engine};
use crate::engine::runner::TurnRequest;
use crate::engine::runner::turn::auto_compact::AutoCompactHook;

#[derive(Default)]
struct CapturingPresenter {
    notices: Mutex<Vec<String>>,
    spinner_messages: Mutex<Vec<String>>,
}

#[async_trait]
impl Presenter for CapturingPresenter {
    fn write_output(&self, _text: &str) {}
    fn print_welcome(&self, _display: &WelcomeDisplay) {}
    fn print_session_status(&self, _display: &SessionStatus) {}
    fn print_notice(&self, text: &str) {
        self.notices.lock().unwrap().push(text.to_string());
    }
    fn print_user_block(&self, _input: &str) {}
    fn print_token(&self, _token: &str) {}
    fn print_thinking_token(&self, _token: &str) {}
    fn finish_tool_line(&self, _line: ToolLine) {}
    fn flush(&self) {}
    fn has_interactive_ui(&self) -> bool {
        false
    }
    fn start_spinner(&self, message: &str) -> ActivityToken {
        self.spinner_messages.lock().unwrap().push(message.to_string());
        ActivityToken::default()
    }
    fn start_tool_spinner(&self, _name: &str, _arguments: &serde_json::Value) -> ActivityToken {
        ActivityToken::default()
    }
    fn start_tool_run(&self, _name: &str, _arguments: &serde_json::Value) {}
    fn stream_port(&self) -> ToolStreamPort {
        ToolStreamPort::default()
    }
    async fn prompt_continue_budget(&self, _max_turns: usize) -> bool {
        false
    }
}

fn echo_tool() -> DynamicTool {
    DynamicTool::new(
        "echo_tool",
        "Echoes a fixed string",
        serde_json::json!({"type": "object", "properties": {}}),
        |_ctx, _args| Box::pin(async { Ok(ToolOutput::text("echoed")) }),
    )
}

fn engine_for(dir: &std::path::Path, model: MockCompletionModel) -> crate::engine::AgentEngine {
    let app_config = Config {
        model: "mock-model".to_string(),
        keep_recent_tokens: 5,
        permission: PermissionConfig { enabled: false },
        auth_file: dir.join("auth.json"),
        ..Config::default()
    };
    mock_engine(
        model,
        MockEngineConfig {
            base_dir: dir,
            app_config,
            session_manager: None,
            built_in_tools: Some(vec![echo_tool()]),
        },
    )
}

async fn seed_history(engine: &crate::engine::AgentEngine) {
    let sid = &engine.session_manager.session_id;
    ConversationMemory::append(
        &engine.session_manager,
        sid,
        vec![Message::user("prior prompt"), Message::assistant("prior response")],
    )
    .await
    .unwrap();
}

async fn assert_compaction_node_present(engine: &crate::engine::AgentEngine) {
    let tree = engine.session_manager.load_tree().await.unwrap();
    let leaf_id = tree.active_leaf_id.as_ref().unwrap();
    assert!(
        tree.ancestor_nodes(leaf_id)
            .iter()
            .any(|n| n.kind == TreeNodeKind::Compaction)
    );
}

#[tokio::test]
async fn test_mid_run_auto_compaction_continues_run_on_compacted_context() {
    let dir = std::env::temp_dir().join(format!("midrun_compact_{}", uuid::Uuid::new_v4()));
    let turn_usage = Usage {
        input_tokens: 120_000,
        output_tokens: 10,
        total_tokens: 120_010,
        ..Default::default()
    };
    let model = MockCompletionModel::from_stream_turns([
        [
            MockStreamEvent::tool_call("t1", "echo_tool", serde_json::json!({})),
            final_event(turn_usage),
        ],
        [MockStreamEvent::text("continued"), final_event(Usage::default())],
    ]);
    let engine = engine_for(&dir, model.clone());
    seed_history(&engine).await;

    let presenter = Arc::new(CapturingPresenter::default());
    let output = engine
        .run_turn(TurnRequest::new("run the echo tool"), presenter.clone())
        .await
        .unwrap();

    // The run continued to completion on the compacted context instead of
    // halting or waiting for the next user prompt.
    assert_eq!(output.status, crate::engine::runner::RunStatus::Completed);
    assert_eq!(output.final_text, "continued");

    // Turn 1, the compaction summarizer, then the resumed model call.
    let requests = model.requests();
    assert_eq!(requests.len(), 3);
    let patched = &requests[2].chat_history;
    // [preamble, compaction summary, kept tree tail, user prompt, assistant
    // tool call, tool result prompt]
    assert_eq!(patched.len(), 6, "summary + kept tree tail + run messages");
    match &patched[1] {
        Message::System { content } => assert!(content.contains("## Goal"), "summary: {content}"),
        other => panic!("expected compaction summary system message, got {other:?}"),
    }
    let as_text = as_text_of;
    assert_eq!(as_text(&patched[2]), "prior response");
    assert_eq!(as_text(&patched[3]), "run the echo tool");
    assert!(as_text(&patched[5]).contains("echoed"));

    assert_compaction_node_present(&engine).await;
    assert!(
        presenter
            .notices
            .lock()
            .unwrap()
            .iter()
            .any(|n| n.contains("Auto-compacted context"))
    );
    assert!(
        presenter
            .spinner_messages
            .lock()
            .unwrap()
            .iter()
            .any(|m| m == "Compacting...")
    );
}

#[tokio::test]
async fn test_mid_run_auto_compaction_falls_back_to_ephemeral_summary_with_pending_checkpoint() {
    let dir = std::env::temp_dir().join(format!("midrun_ephemeral_{}", uuid::Uuid::new_v4()));
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::text("response"), final_event(Usage::default())]]);
    let engine = engine_for(&dir, model);
    seed_history(&engine).await;
    engine
        .session_manager
        .save_checkpoint(vec![Message::user("pending checkpoint work")])
        .await
        .unwrap();
    let usage = Usage {
        input_tokens: 120_000,
        ..Default::default()
    }
    .into();
    engine
        .usage
        .record_turn(crate::engine::tracking::TurnUsage::new(usage, usage), 100);

    let presenter = Arc::new(CapturingPresenter::default());
    let hook = AutoCompactHook::new(
        engine.session_compactor(),
        presenter.clone(),
        engine.usage.clone(),
        engine.context,
        &engine.config.provider,
        engine.config.reserve_tokens,
    );
    let history = vec![
        Message::user("prior prompt"),
        Message::assistant("prior response"),
        Message::user("pending checkpoint work"),
    ];
    let action = hook.handle(&history, &Message::user("latest prompt")).await;

    match action {
        CompletionCallAction::Patch(patch) => {
            let patched = patch.history.expect("patch supplies history");
            match &patched[0] {
                Message::System { content } => assert!(!content.trim().is_empty()),
                other => panic!("expected compaction summary system message, got {other:?}"),
            }
            // Everything from the cut on is kept verbatim, including the
            // pending checkpoint message that the durable store does not have.
            assert_eq!(patched.len(), 2);
            assert!(as_text_of(&patched[1]).contains("pending checkpoint work"));
        }
        other => panic!("expected history patch, got {other:?}"),
    }
    let tree = engine.session_manager.load_tree().await.unwrap();
    let leaf_id = tree.active_leaf_id.as_ref().unwrap();
    assert!(
        !tree
            .ancestor_nodes(leaf_id)
            .iter()
            .any(|n| n.kind == TreeNodeKind::Compaction)
    );
}

fn as_text_of(message: &Message) -> String {
    match message {
        Message::User { content } => content
            .iter()
            .filter_map(|part| match part {
                UserContent::Text(text) => Some(text.text.clone()),
                UserContent::ToolResult(result) => Some(
                    result
                        .content
                        .iter()
                        .filter_map(ToolResultContent::as_text)
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" "),
        Message::Assistant { content, .. } => content
            .iter()
            .filter_map(|part| match part {
                AssistantContent::Text(text) => Some(text.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" "),
        Message::System { content } => content.clone(),
    }
}
