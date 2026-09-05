use crate::ui::interactive::controller::TerminalController;
use crate::ui::interactive::controller::tests::fake::{FakeTerminal, Operation};
use crate::ui::interactive::{InteractiveState, ToolItem, ToolStartRequest, TranscriptItem};

#[test]
fn tool_completion_in_place_preserves_editor_row_and_output_continuity() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

    controller
        .start_tool(ToolStartRequest {
            name: "bash".into(),
            args_summary: "cargo test".into(),
            preview: None,
        })
        .unwrap();
    controller.append_tool_chunk("running 1 test\n").unwrap();
    operations.borrow_mut().clear();

    controller
        .push_transcript_item(TranscriptItem::Tool(ToolItem {
            name: "bash".into(),
            arguments: serde_json::json!({"command": "cargo test"}),
            is_error: false,
            output: "test result: ok".into(),
            output_summary: "ok".into(),
            duration_ms: Some(42),
        }))
        .unwrap();

    assert!(controller.state().active_tool().is_none());
    assert_eq!(controller.transcript().len(), 1);

    let rendered = controller.rendered.as_ref().expect("rendered layout exists");
    assert_eq!(rendered.cursor_row(), 2);
    assert_eq!(rendered.lines.len(), 6);

    operations.borrow_mut().clear();
    controller.write_output("Done.\n").unwrap();

    let ops = operations.borrow();
    assert!(
        ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("Done.")))
    );
}

#[test]
fn consecutive_tools_commit_in_place_without_cumulative_drift() {
    let (backend, _, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

    controller
        .start_tool(ToolStartRequest {
            name: "bash".into(),
            args_summary: "echo one".into(),
            preview: None,
        })
        .unwrap();
    controller
        .push_transcript_item(TranscriptItem::Tool(ToolItem {
            name: "bash".into(),
            arguments: serde_json::json!({"command": "echo one"}),
            is_error: false,
            output: "one".into(),
            output_summary: "one".into(),
            duration_ms: Some(10),
        }))
        .unwrap();

    controller
        .start_tool(ToolStartRequest {
            name: "bash".into(),
            args_summary: "echo two".into(),
            preview: None,
        })
        .unwrap();
    controller
        .push_transcript_item(TranscriptItem::Tool(ToolItem {
            name: "bash".into(),
            arguments: serde_json::json!({"command": "echo two"}),
            is_error: false,
            output: "two".into(),
            output_summary: "two".into(),
            duration_ms: Some(10),
        }))
        .unwrap();

    assert_eq!(controller.transcript().len(), 2);
    assert!(controller.state().active_tool().is_none());
    let rendered = controller.rendered.as_ref().unwrap();
    assert_eq!(rendered.cursor_row(), 2);
    assert_eq!(rendered.lines.len(), 6);
}

#[test]
fn fast_tool_without_active_card_uses_standard_push_path() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    controller
        .push_transcript_item(TranscriptItem::Tool(ToolItem {
            name: "read".into(),
            arguments: serde_json::json!({"path": "src/main.rs"}),
            is_error: false,
            output: "fn main() {}".into(),
            output_summary: "1 line".into(),
            duration_ms: Some(2),
        }))
        .unwrap();

    assert_eq!(controller.transcript().len(), 1);
    let ops = operations.borrow();
    assert!(
        ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("read")))
    );
}
