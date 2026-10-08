use super::super::helpers::{final_event, presenter, request, test_engine};
use crate::config::Config;
use crate::engine::runner::{DisplayEvent, display_events};
use crate::ui::TerminalRenderer;
use rig::completion::Usage;
use rig::streaming::Transcript;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};
use std::collections::HashSet;

#[test]
fn renderer_events_preserve_reasoning_text_order_without_duplicates() {
    let mut reasoning_parts = HashSet::new();
    let transcript = Transcript::parse_prefix(serde_json::json!([
        {"item": "event", "value": {"event": "start", "part": 0, "kind": "reasoning"}},
        {"item": "event", "value": {"event": "reasoning", "part": 0, "text": "think"}},
        {"item": "event", "value": {"event": "end", "part": 0, "content": {"type": "reasoning", "text": "think"}}},
        {"item": "event", "value": {"event": "start", "part": 1, "kind": "text"}},
        {"item": "event", "value": {"event": "text", "part": 1, "text": "answer"}},
        {"item": "event", "value": {"event": "end", "part": 1, "content": {"type": "text", "text": "answer"}}},
    ]))
    .unwrap();
    let events = transcript
        .into_items()
        .into_iter()
        .flat_map(|item| display_events(item, &mut reasoning_parts))
        .collect::<Vec<_>>();

    assert_eq!(
        events,
        [
            DisplayEvent::Reasoning("think".to_string()),
            DisplayEvent::Text("answer".to_string())
        ]
    );
}

#[tokio::test]
async fn final_text_streams_once_and_usage_can_be_unavailable() {
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::text("final text"), final_event(Usage::default())]]);
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
