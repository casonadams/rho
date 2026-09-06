use super::common::HistoryTerminal;
use crate::ui::interactive::{InteractiveState, TerminalController};

fn tool_event(name: &str, output: &str) -> crate::ui::interactive::UiEvent {
    crate::ui::interactive::UiEvent::Transcript(crate::ui::interactive::TranscriptItem::Tool(
        crate::ui::interactive::ToolItem {
            name: name.into(),
            arguments: serde_json::json!({}),
            is_error: false,
            output: output.into(),
            output_summary: "ok".into(),
            duration_ms: Some(1),
        },
    ))
}

#[test]
fn live_batch_flushes_tool_end_with_transcript_without_intermediate_redraw() {
    let mut batch = super::super::batch::LiveBatch::new();
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    controller
        .start_tool(crate::ui::interactive::ToolStartRequest {
            name: "bash".into(),
            args_summary: "cargo test".into(),
            preview: None,
        })
        .unwrap();

    batch
        .enqueue(&mut controller, crate::ui::interactive::UiEvent::ToolEnd)
        .unwrap();
    batch
        .enqueue(&mut controller, tool_event("bash", "all tests passed"))
        .unwrap();

    batch.flush(&mut controller, false).unwrap();
    assert_eq!(controller.transcript().len(), 1);
}

fn push_test_events(batch: &mut super::super::batch::LiveBatch, controller: &mut TerminalController<HistoryTerminal>) {
    let _ = batch.push_event(
        controller,
        crate::ui::interactive::UiEvent::Activity(crate::ui::interactive::Activity::Working),
    );
    let _ = batch.push_event(
        controller,
        crate::ui::interactive::UiEvent::RunningTool(Some("read".into())),
    );
    let _ = batch.push_event(
        controller,
        crate::ui::interactive::UiEvent::Activity(crate::ui::interactive::Activity::Idle),
    );
    assert!(
        batch
            .push_event(controller, tool_event("read", "fn main() {}"))
            .unwrap()
    );
    let _ = batch.push_event(
        controller,
        crate::ui::interactive::UiEvent::Activity(crate::ui::interactive::Activity::Thinking),
    );
}

#[test]
fn live_batch_coalesces_rapid_tool_activity_and_transcript() {
    let mut batch = super::super::batch::LiveBatch::new();
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    controller
        .start_tool(crate::ui::interactive::ToolStartRequest {
            name: "read".into(),
            args_summary: "src/main.rs".into(),
            preview: None,
        })
        .unwrap();

    push_test_events(&mut batch, &mut controller);
    batch.flush(&mut controller, false).unwrap();
    let footer = controller.state().footer();
    let actual = (
        controller.transcript().len(),
        &footer.activity,
        footer.running_tool.as_deref(),
    );
    assert_eq!(actual, (1, &crate::ui::interactive::Activity::Thinking, None));
}
