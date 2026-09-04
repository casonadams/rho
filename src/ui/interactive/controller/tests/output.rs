use super::fake::{FakeTerminal, Operation};
use crate::ui::interactive::controller::TerminalController;
use crate::ui::interactive::controller::ansi::output_cursor;
use crate::ui::interactive::{InteractiveState, OutputEvent, PendingUiBatch, UiEvent};

#[test]
fn output_cursor_tracks_wrap_boundaries_styles_and_wide_text() {
    assert_eq!(output_cursor("123456789", 10), (9, false));
    assert_eq!(output_cursor("1234567890", 10), (0, true));
    assert_eq!(output_cursor("123456789界", 10), (2, false));
    assert_eq!(output_cursor("\u{1b}[2mwide\u{1b}[0m", 10), (4, false));
}

#[test]
fn output_erases_then_writes_then_redraws_with_one_flush() {
    let (backend, operations, _) = FakeTerminal::new(10);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    controller.write_output("answer\nnext").unwrap();

    let operations = operations.borrow();
    let output_index = operations
        .iter()
        .position(|operation| operation == &Operation::Write("answer\r\nnext".into()))
        .unwrap();
    let last_clear = operations
        .iter()
        .rposition(|operation| operation == &Operation::Clear)
        .unwrap();
    let divider_index = operations
        .iter()
        .position(|operation| {
            matches!(
                operation,
                Operation::Write(text) if text.contains("──────────")
            )
        })
        .unwrap();
    assert!(last_clear < output_index);
    assert!(output_index < divider_index);
    assert_eq!(
        operations
            .iter()
            .filter(|operation| operation == &&Operation::Flush)
            .count(),
        1
    );
}

#[test]
fn many_stream_fragments_are_written_with_one_controller_flush() {
    let (backend, operations, _) = FakeTerminal::new(40);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();
    let mut pending = PendingUiBatch::new(16 * 1024);
    for _ in 0..1_000 {
        pending.push(UiEvent::Output(OutputEvent::Text("token".into())));
    }

    controller.write_output(&pending.drain().text).unwrap();

    let operations = operations.borrow();
    assert_eq!(
        operations
            .iter()
            .filter(|operation| operation == &&Operation::Write("token".repeat(1_000)))
            .count(),
        1
    );
    assert_eq!(
        operations
            .iter()
            .filter(|operation| operation == &&Operation::Flush)
            .count(),
        1
    );
}

#[test]
fn streamed_output_resumes_at_the_previous_line_end() {
    let (backend, operations, _) = FakeTerminal::new(10);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    controller.write_output("streamed ").unwrap();
    operations.borrow_mut().clear();
    controller.write_output("response").unwrap();

    let operations = operations.borrow();
    let move_index = operations
        .iter()
        .position(|operation| operation == &Operation::Up(1))
        .unwrap();
    let column_index = operations
        .iter()
        .position(|operation| operation == &Operation::Column(9))
        .unwrap();
    let output_index = operations
        .iter()
        .position(|operation| operation == &Operation::Write("response".into()))
        .unwrap();
    assert!(move_index < column_index);
    assert!(column_index < output_index);
}
