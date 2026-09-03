use super::helpers::{final_event, presenter, request, test_engine};
use crate::config::Config;
use crate::engine::runner::{QUEUED_MESSAGE_BOUNDARY, QueuedMessageBoundary};
use crate::error::AppError;
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

#[tokio::test]
async fn malformed_tool_arguments_are_model_visible_tool_failures() {
    let model = MockCompletionModel::from_stream_turns([
        [
            MockStreamEvent::tool_call("call-1", "read", serde_json::json!({"unexpected": true})),
            final_event(Usage::new()),
        ],
        [MockStreamEvent::text("recovered"), final_event(Usage::new())],
    ]);
    let engine = test_engine(
        model.clone(),
        Config {
            auto_approve: true,
            ..Config::default()
        },
    );
    let output = engine
        .run_turn(request("read"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    assert_eq!(output.tool_failures_count, 1);
    assert!(format!("{:?}", model.requests()[1]).contains("failed to parse tool arguments"));
}

#[tokio::test]
async fn unknown_tool_calls_fail_without_fallback() {
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call("call-1", "unknown", serde_json::json!({})),
        final_event(Usage::new()),
    ]]);
    let engine = test_engine(model, Config::default());
    let error = engine
        .run_turn(request("unknown"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap_err();

    assert!(matches!(error, AppError::InvalidToolCall(name) if name == "unknown"));
}

#[cfg(unix)]
#[tokio::test]
async fn mutating_tools_execute_sequentially() {
    let marker = std::env::temp_dir().join(format!("sequential_marker_{}", uuid::Uuid::new_v4()));
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call(
                "call-1",
                "bash",
                serde_json::json!({"command": format!("sleep 0.05; printf 1 >> {}", marker.display())}),
            ),
            MockStreamEvent::tool_call(
                "call-2",
                "bash",
                serde_json::json!({"command": format!("printf 2 >> {}", marker.display())}),
            ),
            final_event(Usage::new()),
        ],
        vec![MockStreamEvent::text("done"), final_event(Usage::new())],
    ]);
    let engine = test_engine(
        model,
        Config {
            auto_approve: true,
            ..Config::default()
        },
    );
    engine
        .run_turn(request("run"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    assert_eq!(tokio::fs::read_to_string(&marker).await.unwrap(), "12");
    let _ = tokio::fs::remove_file(marker).await;
}

#[cfg(unix)]
#[tokio::test]
async fn cancelled_tool_run_persists_no_incomplete_result() {
    let marker = std::env::temp_dir().join(format!("cancel_marker_{}", uuid::Uuid::new_v4()));
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call(
            "call-1",
            "bash",
            serde_json::json!({"command": format!("sleep 2; touch {}", marker.display())}),
        ),
        final_event(Usage::new()),
    ]]);
    let engine = test_engine(
        model,
        Config {
            auto_approve: true,
            ..Config::default()
        },
    );
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        engine.run_turn(request("run"), presenter(&TerminalRenderer::default())),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    assert!(result.is_err());
    engine.record_cancellation("test interrupt").await.unwrap();
    assert!(!marker.exists());
    let events = engine.session_manager.load_events().await.unwrap();
    assert!(
        !events
            .iter()
            .any(|event| event.kind == crate::session::SessionEventKind::ToolResult)
    );
    assert!(
        events
            .iter()
            .any(|event| event.kind == crate::session::SessionEventKind::Cancellation)
    );
    let summary = events
        .iter()
        .find(|event| event.kind == crate::session::SessionEventKind::RunSummary)
        .unwrap();
    assert_eq!(summary.payload["terminal_status"], "cancelled");
    assert!(engine.session_manager.load_messages().await.unwrap().is_empty());
    let reopened = SessionManager::new(
        engine.session_manager.file_path.parent().unwrap(),
        Some(&engine.session_manager.session_id),
    )
    .unwrap();
    assert!(reopened.load_messages().await.unwrap().is_empty());
}
