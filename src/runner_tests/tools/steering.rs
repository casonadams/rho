use super::super::helpers::{presenter, request, test_engine};
use crate::config::Config;
use crate::engine::runner::{QUEUED_MESSAGE_BOUNDARY, QueuedMessageBoundary};
use crate::ui::TerminalRenderer;
use rho_engine::provider::adapter::{MockAdapter, ModelStreamEvent};
use rho_harness_core::model::{AssistantContent, ChatMessage, ToolCall, ToolFunction, UserContent};
use std::sync::Arc;

async fn assert_steering_event_order(sm: &crate::session::SessionManager) {
    let events = sm.load_events().await.unwrap();
    let tool_res = events
        .iter()
        .position(|e| e.kind == crate::session::SessionEventKind::ToolResult)
        .unwrap();
    let active_res = events
        .iter()
        .position(|e| {
            e.kind == crate::session::SessionEventKind::AssistantResponse
                && e.payload["content"] == "active run complete"
        })
        .unwrap();
    let queued_user = events
        .iter()
        .position(|e| {
            e.kind == crate::session::SessionEventKind::UserMessage && e.payload["prompt"] == "queued steering"
        })
        .unwrap();
    assert!(tool_res < active_res && active_res < queued_user);
}

#[tokio::test]
async fn queued_steering_is_delivered_after_the_active_tool_run_completes() {
    assert_eq!(QUEUED_MESSAGE_BOUNDARY, QueuedMessageBoundary::ActiveRunCompleted);
    let model = Arc::new(MockAdapter::default());
    model.add_turn(vec![Ok(ModelStreamEvent::ToolCall(ToolCall::new(
        "call-1",
        ToolFunction::new("read", serde_json::json!({"path": "missing"})),
    )))]);
    model.add_turn(vec![Ok(ModelStreamEvent::Text("active run complete".to_string()))]);
    model.add_turn(vec![Ok(ModelStreamEvent::Text("queued response".to_string()))]);

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

    assert_steering_event_order(&engine.session_manager).await;
    let encoded = serde_json::to_string(&model.requests()[2].messages).unwrap();
    assert!(encoded.contains("active run complete") && encoded.contains("queued steering"));
}

fn count_calls_and_results(history: &[ChatMessage]) -> (usize, usize) {
    let mut calls = 0;
    let mut results = 0;
    for m in history {
        match m {
            ChatMessage::Assistant { content, .. } => {
                calls += content
                    .iter()
                    .filter(|c| matches!(c, AssistantContent::ToolCall(_)))
                    .count();
            }
            ChatMessage::User { content } => {
                results += content
                    .iter()
                    .filter(|c| matches!(c, UserContent::ToolResult(_)))
                    .count();
            }
            _ => {}
        }
    }
    (calls, results)
}

#[tokio::test]
async fn one_tool_round_preserves_canonical_call_and_one_result() {
    let model = Arc::new(MockAdapter::default());
    model.add_turn(vec![Ok(ModelStreamEvent::ToolCall(ToolCall::new(
        "call-1",
        ToolFunction::new("read", serde_json::json!({"path": "missing"})),
    )))]);
    model.add_turn(vec![Ok(ModelStreamEvent::Text("done".to_string()))]);

    let engine = test_engine(model.clone(), Config::default());
    let output = engine
        .run_turn(request("read"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    assert_eq!(output.tool_calls_count, 1);
    assert_eq!(count_calls_and_results(&model.requests()[1].messages), (1, 1));
}

#[tokio::test]
async fn multiple_tool_calls_have_one_correlated_result_each() {
    let model = Arc::new(MockAdapter::default());
    model.add_turn(vec![
        Ok(ModelStreamEvent::ToolCall(ToolCall::new(
            "call-1",
            ToolFunction::new("read", serde_json::json!({"path": "missing-a"})),
        ))),
        Ok(ModelStreamEvent::ToolCall(ToolCall::new(
            "call-2",
            ToolFunction::new("read", serde_json::json!({"path": "missing-b"})),
        ))),
    ]);
    model.add_turn(vec![Ok(ModelStreamEvent::Text("done".to_string()))]);

    let engine = test_engine(model.clone(), Config::default());
    let output = engine
        .run_turn(request("read both"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    assert_eq!(output.tool_calls_count, 2);
    assert_eq!(output.tool_failures_count, 2);
    let req = &model.requests()[1];
    let serialized = serde_json::to_value(&req.messages).unwrap();
    let calls = serialized.to_string().matches("toolcall").count();
    let results = serialized.to_string().matches("toolresult").count();
    assert_eq!((calls, results), (2, 2));
}
