use super::helpers::{final_event, presenter, request, terminal_session, test_engine};
use crate::config::Config;
use crate::engine::runner::{DisplayEvent, TerminalApprovalSink, TerminalSinkConfig, display_events};
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{Activity, InteractiveUi, UiEvent};
use rig::completion::Usage;
use rig::streaming::StreamedAssistantContent;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};
use std::collections::HashSet;

#[test]
fn renderer_events_preserve_reasoning_text_order_without_duplicates() {
    let mut reasoning_parts = HashSet::new();
    let events = [
        StreamedAssistantContent::ReasoningDelta {
            id: "reasoning-1".to_string(),
            provider_id: None,
            reasoning: "think".to_string(),
        },
        StreamedAssistantContent::Reasoning {
            id: "reasoning-1".to_string(),
            reasoning: rig::message::Reasoning::new("think"),
        },
        StreamedAssistantContent::text("answer"),
    ]
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

#[test]
fn visible_stream_clears_spinner_and_hidden_output_resumes_it() {
    let renderer = TerminalRenderer::default();
    let sink = TerminalApprovalSink::new(
        &presenter(&renderer),
        TerminalSinkConfig {
            model_label: "model".to_string(),
            auto_approve: true,
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        terminal_session(),
    );
    sink.emit_reasoning("think ");
    sink.emit_reasoning("harder");
    assert_eq!(sink.state.lock().unwrap().reasoning.join(""), "think harder");
    assert!(sink.state.lock().unwrap().spinner.is_none());

    sink.emit_text("answer");
    assert!(sink.state.lock().unwrap().reasoning.is_empty());
    assert!(sink.state.lock().unwrap().spinner.is_none());

    sink.resume_model_spinner();
    assert!(sink.state.lock().unwrap().spinner.is_some());
    sink.finish_spinner();
}

#[test]
fn interactive_sink_uses_footer_activity_instead_of_a_progress_bar() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);
    let sink = TerminalApprovalSink::new(
        &presenter(&renderer),
        TerminalSinkConfig {
            model_label: "model".to_string(),
            auto_approve: true,
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        terminal_session(),
    );

    assert!(sink.state.lock().unwrap().spinner.is_some());
    sink.tool_start("read", &serde_json::json!({"path": "src/lib.rs"}));

    let activities = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event {
            UiEvent::Activity(activity) => Some(activity),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(activities, [Activity::Thinking, Activity::Idle, Activity::Working]);
}

#[test]
fn interactive_stream_preserves_spinner_until_finished() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);
    let sink = TerminalApprovalSink::new(
        &presenter(&renderer),
        TerminalSinkConfig {
            model_label: "model".to_string(),
            auto_approve: true,
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        terminal_session(),
    );

    assert!(sink.state.lock().unwrap().spinner.is_some());

    sink.emit_reasoning("thinking about solution");
    assert!(sink.state.lock().unwrap().spinner.is_some());

    sink.emit_text("Here is the answer");
    assert!(sink.state.lock().unwrap().spinner.is_some());

    sink.emit_text(" and more details");
    assert!(sink.state.lock().unwrap().spinner.is_some());

    sink.finish_spinner();
    assert!(sink.state.lock().unwrap().spinner.is_none());

    let activities = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event {
            UiEvent::Activity(activity) => Some(activity),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(activities, [Activity::Thinking, Activity::Idle]);
}

#[test]
fn reasoning_flushes_before_tool_classification() {
    let renderer = TerminalRenderer::default();
    let sink = TerminalApprovalSink::new(
        &presenter(&renderer),
        TerminalSinkConfig {
            model_label: "model".to_string(),
            auto_approve: false,
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        terminal_session(),
    );
    sink.emit_reasoning("pondering next step");
    assert_eq!(sink.state.lock().unwrap().reasoning.join(""), "pondering next step");

    sink.tool_start("bash", &serde_json::json!({ "command": "cargo test" }));

    assert!(sink.state.lock().unwrap().reasoning.is_empty());
}

#[tokio::test]
async fn final_text_streams_once_and_usage_can_be_unavailable() {
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::text("final text"), final_event(Usage::new())]]);
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
