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

#[tokio::test]
async fn streamed_write_arguments_open_and_fill_the_live_card_before_the_write_runs() {
    use crate::ui::interactive::{InteractiveUi, UiEvent};
    use rho_engine::provider::adapter::ModelStreamEvent;
    use rho_harness_core::model::{ToolCall, ToolFunction};

    let delta = |json: &str| ModelStreamEvent::ToolInputDelta {
        id: "call-1".into(),
        name: "write".into(),
        json: json.into(),
    };
    let model = Arc::new(MockAdapter::from_turns([
        vec![
            ModelStreamEvent::Text("Writing it now".into()),
            delta(""),
            delta(r#"{"content": "alpha\n"#),
            delta(r#"beta\n", "path": "notes.txt"}"#),
            ModelStreamEvent::ToolCall(ToolCall::new(
                "call-1",
                ToolFunction::new(
                    "write",
                    serde_json::json!({"path": "notes.txt", "content": "alpha\nbeta\n"}),
                ),
            )),
        ],
        vec![ModelStreamEvent::Text("done".into())],
    ]));
    let (ui, mut events) = InteractiveUi::channel();
    let engine = test_engine(model, Config::default());
    engine
        .run_turn(request("write"), presenter(&TerminalRenderer::with_ui(ui)))
        .await
        .unwrap();

    let mut log = Vec::new();
    while let Ok(event) = events.try_recv() {
        match event {
            UiEvent::ToolStart(r) => log.push(format!("start {}", r.name)),
            UiEvent::ToolChunk { chunk } => log.push(format!("chunk {chunk}")),
            UiEvent::Transcript(crate::ui::interactive::TranscriptItem::Tool(_)) => log.push("committed".into()),
            _ => {}
        }
    }
    assert_eq!(
        log,
        [
            "start write",
            "chunk alpha\n",
            "start write",
            "chunk alpha\nbeta\n",
            "start write",
            "committed",
        ],
        "the card is refreshed with the call's final arguments when it runs"
    );
}
