use super::super::helpers::{presenter, terminal_session};
use crate::engine::runner::{TerminalApprovalSink, TerminalSinkConfig};
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{Activity, InteractiveUi, UiEvent};

fn sample_test_sink(renderer: &TerminalRenderer) -> std::sync::Arc<TerminalApprovalSink> {
    TerminalApprovalSink::new(
        &presenter(renderer),
        TerminalSinkConfig {
            model_label: "model".to_string(),
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        terminal_session(),
    )
}

fn collect_activities(events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>) -> Vec<Activity> {
    std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|e| match e {
            UiEvent::Activity(a) => Some(a),
            _ => None,
        })
        .collect()
}

#[test]
fn visible_stream_clears_spinner_and_hidden_output_resumes_it() {
    let renderer = TerminalRenderer::default();
    let sink = sample_test_sink(&renderer);
    sink.emit_reasoning("think ");
    sink.emit_reasoning("harder");
    assert_eq!(sink.state.lock().unwrap().reasoning.join(""), "think harder");

    sink.emit_text("answer");
    assert!(sink.state.lock().unwrap().reasoning.is_empty());

    sink.resume_model_spinner();
    assert!(sink.state.lock().unwrap().spinner.is_some());
    sink.finish_spinner();
}

#[test]
fn interactive_sink_uses_footer_activity_instead_of_a_progress_bar() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);
    let sink = sample_test_sink(&renderer);

    assert!(sink.state.lock().unwrap().spinner.is_some());
    sink.tool_start("read", &serde_json::json!({"path": "src/lib.rs"}));
    assert_eq!(
        collect_activities(&mut events),
        [Activity::Thinking, Activity::Idle, Activity::Working]
    );
}

#[test]
fn interactive_stream_preserves_spinner_until_finished() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);
    let sink = sample_test_sink(&renderer);

    sink.emit_reasoning("thinking about solution");
    sink.emit_text("Here is the answer");
    sink.emit_text(" and more details");
    assert!(sink.state.lock().unwrap().spinner.is_some());

    sink.finish_spinner();
    assert!(sink.state.lock().unwrap().spinner.is_none());
    assert_eq!(collect_activities(&mut events), [Activity::Thinking, Activity::Idle]);
}

#[test]
fn reasoning_flushes_before_tool_classification() {
    let renderer = TerminalRenderer::default();
    let sink = TerminalApprovalSink::new(
        &presenter(&renderer),
        TerminalSinkConfig {
            model_label: "model".to_string(),
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        terminal_session(),
    );
    sink.emit_reasoning("pondering next step");
    assert_eq!(sink.state.lock().unwrap().reasoning.join(""), "pondering next step");

    sink.tool_start("bash", &serde_json::json!({ "command": "cargo test" }));

    assert!(sink.state.lock().unwrap().reasoning.is_empty());
}

#[test]
fn reasoning_flush_emits_transcript_thinking_item() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);
    let sink = sample_test_sink(&renderer);

    sink.emit_reasoning("analyzing the architecture");
    sink.emit_text("Here is the answer");

    let items = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|e| match e {
            UiEvent::Transcript(item) => Some(item),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        items,
        vec![crate::ui::interactive::TranscriptItem::Thinking(
            "analyzing the architecture".into()
        )]
    );
}
