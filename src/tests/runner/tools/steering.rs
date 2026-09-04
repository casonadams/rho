use super::super::helpers::{final_event, presenter, request, test_engine};
use crate::config::Config;
use crate::engine::runner::{QUEUED_MESSAGE_BOUNDARY, QueuedMessageBoundary};
use crate::ui::TerminalRenderer;
use rig::completion::Usage;
use rig::message::{AssistantContent, Message, UserContent};
use rig::test_utils::{MockCompletionModel, MockStreamEvent};

#[tokio::test]
async fn queued_steering_is_delivered_after_the_active_tool_run_completes() {
    assert_eq!(QUEUED_MESSAGE_BOUNDARY, QueuedMessageBoundary::ActiveRunCompleted);
    let model = MockCompletionModel::from_stream_turns([
        [
            MockStreamEvent::tool_call("call-1", "read", serde_json::json!({"path": "missing"})),
            final_event(Usage::new()),
        ],
        [MockStreamEvent::text("active run complete"), final_event(Usage::new())],
        [MockStreamEvent::text("queued response"), final_event(Usage::new())],
    ]);
    let engine = test_engine(model.clone(), Config::default());

    engine
        .run_turn(request("active prompt"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();
    assert_eq!(model.requests().len(), 2);
    engine
        .run_turn(request("queued steering"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    let events = engine.session_manager.load_events().await.unwrap();
    let tool_result = events
        .iter()
        .position(|event| event.kind == crate::session::SessionEventKind::ToolResult)
        .unwrap();
    let active_response = events
        .iter()
        .position(|event| {
            event.kind == crate::session::SessionEventKind::AssistantResponse
                && event.payload["content"] == "active run complete"
        })
        .unwrap();
    let queued_user = events
        .iter()
        .position(|event| {
            event.kind == crate::session::SessionEventKind::UserMessage && event.payload["prompt"] == "queued steering"
        })
        .unwrap();
    assert!(tool_result < active_response);
    assert!(active_response < queued_user);

    let queued_request = &model.requests()[2].chat_history;
    let encoded = serde_json::to_string(queued_request).unwrap();
    assert!(encoded.contains("active run complete"));
    assert!(encoded.contains("queued steering"));
}

#[tokio::test]
async fn one_tool_round_preserves_canonical_call_and_one_result() {
    let model = MockCompletionModel::from_stream_turns([
        [
            MockStreamEvent::tool_call("call-1", "read", serde_json::json!({"path": "missing"})),
            final_event(Usage::new()),
        ],
        [MockStreamEvent::text("done"), final_event(Usage::new())],
    ]);
    let engine = test_engine(model.clone(), Config::default());
    let output = engine
        .run_turn(request("read"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    assert_eq!(output.tool_calls_count, 1);
    let req = &model.requests()[1];
    let assistant_calls = req
        .chat_history
        .iter()
        .filter_map(|message| match message {
            Message::Assistant { content, .. } => Some(
                content
                    .iter()
                    .filter(|content| matches!(content, AssistantContent::ToolCall(_)))
                    .count(),
            ),
            _ => None,
        })
        .sum::<usize>();
    let results = req
        .chat_history
        .iter()
        .filter_map(|message| match message {
            Message::User { content } => Some(
                content
                    .iter()
                    .filter(|content| matches!(content, UserContent::ToolResult(_)))
                    .count(),
            ),
            _ => None,
        })
        .sum::<usize>();
    assert_eq!((assistant_calls, results), (1, 1));
}

#[tokio::test]
async fn multiple_tool_calls_have_one_correlated_result_each() {
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("call-1", "read", serde_json::json!({"path": "missing-a"})),
            MockStreamEvent::tool_call("call-2", "read", serde_json::json!({"path": "missing-b"})),
            final_event(Usage::new()),
        ],
        vec![MockStreamEvent::text("done"), final_event(Usage::new())],
    ]);
    let engine = test_engine(model.clone(), Config::default());
    let output = engine
        .run_turn(request("read both"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    assert_eq!(output.tool_calls_count, 2);
    assert_eq!(output.tool_failures_count, 2);
    let req = &model.requests()[1];
    let serialized = serde_json::to_value(&req.chat_history).unwrap();
    let calls = serialized.to_string().matches("toolcall").count();
    let results = serialized.to_string().matches("toolresult").count();
    assert_eq!((calls, results), (2, 2));
}
