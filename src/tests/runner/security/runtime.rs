use super::super::helpers::{final_event, presenter, request, test_engine};
use crate::auth::AuthStore;
use crate::config::Config;
use crate::engine::runner::RunStatus;
use crate::error::AppError;
use crate::ui::TerminalRenderer;
use rig::completion::{FinishReason, Usage};
use rig::test_utils::{MockCompletionModel, MockStreamEvent};

#[tokio::test]
async fn normalized_usage_is_exposed_when_available() {
    let usage = Usage {
        input_tokens: 10,
        output_tokens: 4,
        total_tokens: 14,
        cached_input_tokens: 3,
        cache_creation_input_tokens: 2,
        tool_use_prompt_tokens: 1,
        reasoning_tokens: 2,
    };
    let model = MockCompletionModel::from_stream_turns([[MockStreamEvent::text("done"), final_event(usage)]]);
    let engine = test_engine(model, Config::default());
    let output = engine
        .run_turn(request("prompt"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    assert_eq!(output.usage, Some(usage.into()));
    assert!(output.metrics.usage_available);
    assert_eq!(output.metrics.usage.unwrap().cached_input_tokens, Some(3));
    assert_eq!(output.metrics.usage.unwrap().reasoning_tokens, Some(2));
    assert_eq!(engine.context_usage_display(), "15/200k (0%)");
}

#[tokio::test]
async fn content_filter_finish_is_distinct() {
    let final_record =
        rig::streaming::StreamFinal::new("mock", Usage::new()).with_finish_reason(FinishReason::ContentFilter);
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::text("filtered partial"),
        MockStreamEvent::FinalResponse(final_record),
    ]]);
    let engine = test_engine(model, Config::default());
    let output = engine
        .run_turn(request("prompt"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    assert_eq!(output.status, RunStatus::ContentFiltered);
}

#[tokio::test]
async fn explicit_output_limit_and_max_turn_budget_reach_rig() {
    let config = Config {
        max_output_tokens: Some(321),
        max_turns: 1,
        ..Config::default()
    };
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call("call-1", "read", serde_json::json!({"path": "missing"})),
        final_event(Usage::new()),
    ]]);
    let engine = test_engine(model.clone(), config);
    let error = engine
        .run_turn(request("read"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap_err();

    assert!(matches!(error, AppError::ModelBudgetExhausted { max_turns: 1 }));
    assert_eq!(model.requests()[0].max_tokens, Some(321));
}

#[test]
fn auth_store_type_remains_constructible_for_public_engine_api() {
    let _ = AuthStore::default();
}
