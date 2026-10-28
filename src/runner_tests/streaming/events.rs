use super::super::helpers::{presenter, request, test_engine};
use crate::config::Config;
use crate::engine::runner::{DisplayEvent, display_events};
use crate::ui::TerminalRenderer;
use rho_engine::engine::runner::StreamEvent;
use rho_engine::provider::adapter::MockAdapter;
use std::sync::Arc;

#[test]
fn renderer_events_preserve_reasoning_text_order_without_duplicates() {
    let e1 = display_events(&StreamEvent::Reasoning("think".to_string()));
    let e2 = display_events(&StreamEvent::Text("answer".to_string()));
    assert_eq!(e1, vec![DisplayEvent::Reasoning("think".to_string())]);
    assert_eq!(e2, vec![DisplayEvent::Text("answer".to_string())]);
}

#[tokio::test]
async fn final_text_streams_once_and_usage_can_be_unavailable() {
    let model = Arc::new(MockAdapter::text("final text"));
    let engine = test_engine(model, Config::default());
    let output = engine
        .run_turn(request("prompt"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();
    assert_eq!(output.final_text, "final text");
    assert_eq!(output.usage, None);
    assert_eq!(output.requests, 1);
    assert!(!output.metrics.usage_available);
    assert_eq!(output.metrics.model_turns, 1);
}
