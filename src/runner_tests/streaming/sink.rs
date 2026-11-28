use super::super::helpers::{drain_ui_events, presenter, terminal_session};
use crate::engine::runner::{TerminalApprovalSink, TerminalSinkConfig};
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{Activity, InteractiveUi, UiEvent};

fn sample_test_sink(renderer: &TerminalRenderer) -> (std::sync::Arc<TerminalApprovalSink>, rho_test_support::TestDir) {
    let (session, dir) = terminal_session();
    let sink = TerminalApprovalSink::new(
        &presenter(renderer),
        TerminalSinkConfig {
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        session,
    );
    (sink, dir)
}

fn collect_activities(events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>) -> Vec<Activity> {
    drain_ui_events(events, |event| match event {
        UiEvent::Activity(a) => Some(a),
        _ => None,
    })
}

#[test]
fn visible_stream_clears_spinner_and_hidden_output_resumes_it() {
    let renderer = TerminalRenderer::default();
    let (sink, _dir) = sample_test_sink(&renderer);
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
    let (sink, _dir) = sample_test_sink(&renderer);

    assert!(sink.state.lock().unwrap().spinner.is_some());
    sink.tool_start("read", &serde_json::json!({"path": "src/lib.rs"}));
    assert_eq!(collect_activities(&mut events), [Activity::Thinking, Activity::Working]);

    sink.tool_finished(crate::engine::runner::ToolFinishDetails {
        name: "read",
        arguments: &serde_json::json!({"path": "src/lib.rs"}),
        output: "file contents",
        is_error: false,
    });
    assert_eq!(collect_activities(&mut events), [Activity::Thinking]);

    sink.finish_spinner();
    assert_eq!(collect_activities(&mut events), [Activity::Idle]);
}

#[test]
fn interactive_stream_preserves_spinner_until_finished() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);
    let (sink, _dir) = sample_test_sink(&renderer);

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
    let (session, _dir) = terminal_session();
    let sink = TerminalApprovalSink::new(
        &presenter(&renderer),
        TerminalSinkConfig {
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        session,
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
    let (sink, _dir) = sample_test_sink(&renderer);

    sink.emit_reasoning("analyzing the architecture");
    sink.emit_text("Here is the answer");

    let items = drain_ui_events(&mut events, |event| match event {
        UiEvent::Transcript(item) => Some(item),
        _ => None,
    });

    assert_eq!(
        items,
        vec![crate::ui::interactive::TranscriptItem::Thinking(
            "analyzing the architecture".into()
        )]
    );
}
