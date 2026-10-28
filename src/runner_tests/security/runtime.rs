use super::super::helpers::{presenter, request, test_engine};
use crate::auth::AuthStore;
use crate::config::Config;
use crate::engine::runner::RunStatus;
use crate::ui::TerminalRenderer;
use rho_engine::provider::adapter::{FinishReason, MockAdapter, ModelStreamEvent};
use std::sync::Arc;

#[tokio::test]
async fn normalized_usage_is_exposed_when_available() {
    let usage = rho_engine::engine::metrics::StructuralUsage {
        input_tokens: 10,
        output_tokens: 4,
        total_tokens: 14,
        cached_input_tokens: Some(3),
        cache_creation_input_tokens: Some(2),
        tool_use_prompt_tokens: None,
        reasoning_tokens: Some(2),
    };
    let model = Arc::new(MockAdapter::default());
    model.add_turn(vec![
        Ok(ModelStreamEvent::Text("done".to_string())),
        Ok(ModelStreamEvent::Usage(usage)),
        Ok(ModelStreamEvent::Finish(FinishReason::Stop)),
    ]);
    let engine = test_engine(model, Config::default());
    let output = engine
        .run_turn(request("prompt"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    assert_eq!(output.usage, Some(usage));
    assert!(output.metrics.usage_available);
    assert_eq!(output.metrics.usage.unwrap().cached_input_tokens, Some(3));
    assert_eq!(output.metrics.usage.unwrap().reasoning_tokens, Some(2));
}

#[tokio::test]
async fn content_filter_finish_is_distinct() {
    let model = Arc::new(MockAdapter::default());
    model.add_turn(vec![
        Ok(ModelStreamEvent::Text("filtered partial".to_string())),
        Ok(ModelStreamEvent::Finish(FinishReason::ContentFilter)),
    ]);
    let engine = test_engine(model, Config::default());
    let output = engine
        .run_turn(request("prompt"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    assert_eq!(output.status, RunStatus::ContentFiltered);
}

#[test]
fn auth_store_type_remains_constructible_for_public_engine_api() {
    let _ = AuthStore::default();
}
