use super::super::helpers::{final_event, presenter, request, test_engine};
use crate::config::Config;
use crate::error::AppError;
use crate::ui::TerminalRenderer;
use rig::completion::Usage;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};

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
