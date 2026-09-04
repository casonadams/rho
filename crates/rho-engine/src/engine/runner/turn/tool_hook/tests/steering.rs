use super::super::TurnToolExecutionHook;
use super::super::steering::{
    STEERING_SKIP_REASON, attach_steering_to_output, format_steering_message, format_steering_messages,
};
use crate::engine::runner::sink::{TerminalApprovalSink, TerminalSinkConfig};
use crate::engine::runner::turn::types::SteeringQueueProvider;
use async_trait::async_trait;
use rho_harness_core::session::SessionManager;
use rig::agent::AgentBuilder;
use rig::completion::message::{AssistantContent, ToolCall, ToolFunction};
use rig::test_utils::{MockCompletionModel, MockTurn};
use serde_json::json;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct MockSteeringQueue {
    messages: Mutex<Vec<String>>,
}

impl MockSteeringQueue {
    fn new(messages: &[&str]) -> Self {
        Self {
            messages: Mutex::new(messages.iter().map(|s| (*s).to_string()).collect()),
        }
    }

    fn enqueue(&self, msg: &str) {
        self.messages.lock().unwrap().push(msg.to_string());
    }
}

#[async_trait]
impl SteeringQueueProvider for MockSteeringQueue {
    async fn poll_steering(&self) -> Vec<String> {
        let mut guard = self.messages.lock().unwrap();
        std::mem::take(&mut *guard)
    }
}

fn mock_sink() -> Arc<TerminalApprovalSink> {
    let temp_dir = std::env::temp_dir().join(format!("sink_test_{}", uuid::Uuid::new_v4()));
    let session = SessionManager::new(&temp_dir, None).unwrap();
    TerminalApprovalSink::new(
        &crate::engine::eval::presenter::presenter(),
        TerminalSinkConfig {
            model_label: "test-model".to_string(),
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        session,
    )
}

fn batched_tool_calls(calls: Vec<(&str, &str, serde_json::Value)>) -> MockTurn {
    let contents = calls.into_iter().map(|(id, name, args)| {
        AssistantContent::ToolCall(ToolCall::from_wire(id, ToolFunction::new(name.to_string(), args)))
    });
    MockTurn::from_contents(contents)
}

#[test]
fn test_steering_format_and_attach() {
    let msg = format_steering_message("stop editing");
    assert!(msg.starts_with("[USER STEERING INTERRUPT]:\nstop editing"));
    assert!(msg.contains("Please adjust your approach immediately"));

    let combined = format_steering_messages(&["one".to_string(), "two".to_string()]);
    assert!(combined.contains("one\n\ntwo"));

    let attached = attach_steering_to_output("tool output", &msg);
    assert_eq!(attached, format!("tool output\n\n{msg}"));

    let attached_empty = attach_steering_to_output("", &msg);
    assert_eq!(attached_empty, msg);
}

#[tokio::test]
async fn test_steering_during_tool_execution_augments_result_and_skips_next() {
    let dir = tempfile::tempdir().unwrap();
    let file_a = dir.path().join("a.txt");
    let file_b = dir.path().join("b.txt");
    tokio::fs::write(&file_a, "file a content").await.unwrap();

    let steering = Arc::new(MockSteeringQueue::default());
    let hook = TurnToolExecutionHook::new(mock_sink(), "anthropic", Some(steering.clone()));

    // Stage steering message so it is drained when tool 1 completes
    steering.enqueue("pivot to another task");

    let model = MockCompletionModel::new([
        batched_tool_calls(vec![
            ("1", "read", json!({"path": file_a})),
            ("2", "write", json!({"path": file_b, "content": "hello"})),
        ]),
        MockTurn::text("acknowledged steering"),
    ]);

    let agent = AgentBuilder::new(model.clone())
        .tool(crate::tools::ReadTool::new(dir.path()))
        .tool(crate::tools::WriteTool::new(dir.path()))
        .add_hook(hook)
        .record_content_telemetry(false)
        .build();

    let response = agent.runner("start").max_turns(5).run().await.unwrap();
    assert_eq!(response.output, "acknowledged steering");

    // file_b should never have been created because tool 2 was skipped
    assert!(!file_b.exists());

    // Chat history for turn 2 should contain the steering interrupt and skip reason
    let requests = model.requests();
    assert!(requests.len() >= 2);
    let history_str = format!("{:?}", requests[1].chat_history);
    assert!(history_str.contains("[USER STEERING INTERRUPT]"));
    assert!(history_str.contains("pivot to another task"));
    assert!(history_str.contains(STEERING_SKIP_REASON));
}

#[tokio::test]
async fn test_steering_before_tool_call_skips_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let file_b = dir.path().join("b.txt");

    let steering = Arc::new(MockSteeringQueue::new(&["abort initial tool"]));
    let hook = TurnToolExecutionHook::new(mock_sink(), "anthropic", Some(steering));

    let model = MockCompletionModel::new([
        MockTurn::tool_call("1", "write", json!({"path": file_b, "content": "hello"})),
        MockTurn::text("tool skipped"),
    ]);

    let agent = AgentBuilder::new(model.clone())
        .tool(crate::tools::WriteTool::new(dir.path()))
        .add_hook(hook)
        .record_content_telemetry(false)
        .build();

    let response = agent.runner("start").max_turns(3).run().await.unwrap();
    assert_eq!(response.output, "tool skipped");
    assert!(!file_b.exists());

    let requests = model.requests();
    let history_str = format!("{:?}", requests[1].chat_history);
    assert!(history_str.contains(STEERING_SKIP_REASON));
    assert!(history_str.contains("abort initial tool"));
}

#[tokio::test]
async fn test_no_steering_allows_all_tools_to_run() {
    let dir = tempfile::tempdir().unwrap();
    let file_a = dir.path().join("a.txt");
    let file_b = dir.path().join("b.txt");
    tokio::fs::write(&file_a, "file a content").await.unwrap();

    let steering = Arc::new(MockSteeringQueue::default());
    let hook = TurnToolExecutionHook::new(mock_sink(), "anthropic", Some(steering));

    let model = MockCompletionModel::new([
        batched_tool_calls(vec![
            ("1", "read", json!({"path": file_a})),
            ("2", "write", json!({"path": file_b, "content": "created"})),
        ]),
        MockTurn::text("all tools done"),
    ]);

    let agent = AgentBuilder::new(model.clone())
        .tool(crate::tools::ReadTool::new(dir.path()))
        .tool(crate::tools::WriteTool::new(dir.path()))
        .add_hook(hook)
        .record_content_telemetry(false)
        .build();

    let response = agent.runner("start").max_turns(5).run().await.unwrap();
    assert_eq!(response.output, "all tools done");
    assert!(file_b.exists());
    let content = tokio::fs::read_to_string(&file_b).await.unwrap();
    assert_eq!(content, "created");
}
