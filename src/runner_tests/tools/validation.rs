use super::super::helpers::{presenter, request, test_engine};
use crate::config::Config;
use crate::ui::TerminalRenderer;
use rho_engine::provider::adapter::{MockAdapter, ModelStreamEvent};
use rho_harness_core::model::{ToolCall, ToolFunction};
use std::sync::Arc;

#[tokio::test]
async fn malformed_tool_arguments_are_model_visible_tool_failures() {
    let model = Arc::new(MockAdapter::default());
    model.add_turn(vec![Ok(ModelStreamEvent::ToolCall(ToolCall::new(
        "call-1",
        ToolFunction::new("read", serde_json::json!({"unexpected": true})),
    )))]);
    model.add_turn(vec![Ok(ModelStreamEvent::Text("recovered".to_string()))]);

    let engine = test_engine(model.clone(), Config::default());
    let output = engine
        .run_turn(request("read"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    assert_eq!(output.tool_failures_count, 1);
}
