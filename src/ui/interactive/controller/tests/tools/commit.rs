use crate::ui::interactive::controller::TerminalController;
use crate::ui::interactive::controller::tests::fake::{FakeTerminal, Operation};
use crate::ui::interactive::{InteractiveState, ToolItem, ToolStartRequest, TranscriptItem};

fn bash_tool_item(cmd: &str, output: &str) -> TranscriptItem {
    TranscriptItem::Tool(ToolItem {
        name: "bash".into(),
        arguments: serde_json::json!({"command": cmd}),
        is_error: false,
        output: output.into(),
        output_summary: output.into(),
        duration_ms: Some(10),
    })
}

fn start_bash_tool(controller: &mut TerminalController<FakeTerminal>, cmd: &str) {
    controller
        .start_tool(ToolStartRequest {
            name: "bash".into(),
            args_summary: cmd.into(),
            preview: None,
        })
        .unwrap();
}

#[test]
fn tool_completion_in_place_preserves_editor_row_and_output_continuity() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

    start_bash_tool(&mut controller, "cargo test");
    controller.append_tool_chunk("running 1 test\n").unwrap();
    operations.borrow_mut().clear();

    controller
        .push_transcript_item(bash_tool_item("cargo test", "ok"))
        .unwrap();
    assert!(controller.state().active_tool().is_none() && controller.transcript().len() == 1);

    let rendered = controller.rendered.as_ref().expect("rendered layout exists");
    assert_eq!((rendered.cursor_row(), rendered.lines.len()), (3, 7));

    operations.borrow_mut().clear();
    controller.write_output("Done.\n").unwrap();
    assert!(
        operations
            .borrow()
            .iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("Done.")))
    );
}

#[test]
fn consecutive_tools_commit_in_place_without_cumulative_drift() {
    let (backend, _, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

    start_bash_tool(&mut controller, "echo one");
    controller
        .push_transcript_item(bash_tool_item("echo one", "one"))
        .unwrap();

    start_bash_tool(&mut controller, "echo two");
    controller
        .push_transcript_item(bash_tool_item("echo two", "two"))
        .unwrap();

    assert_eq!(
        (
            controller.transcript().len(),
            controller.state().active_tool().is_none()
        ),
        (2, true)
    );
    let rendered = controller.rendered.as_ref().unwrap();
    assert_eq!((rendered.cursor_row(), rendered.lines.len()), (3, 7));
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

#[test]
fn committed_tool_preserves_blank_line_separation_from_preceding_block() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

    controller
        .push_transcript_item(TranscriptItem::UserMessage("run tests".into()))
        .unwrap();

    start_bash_tool(&mut controller, "cargo test");
    operations.borrow_mut().clear();

    controller
        .push_transcript_item(bash_tool_item("cargo test", "ok"))
        .unwrap();

    let ops = operations.borrow();
    assert!(
        ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.is_empty()))
    );
}
