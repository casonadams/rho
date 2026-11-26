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

fn assert_operation_order(operations: &[Operation]) {
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
    assert!(last_clear < output_index && output_index < divider_index);
}

#[test]
fn output_erases_then_writes_then_redraws_with_one_flush() {
    let (backend, operations, _) = FakeTerminal::new(10);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    controller.write_output("answer\nnext").unwrap();
    let ops = operations.borrow();
    assert_operation_order(&ops);
    assert_eq!(
        ops.iter().filter(|operation| operation == &&Operation::Flush).count(),
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

    controller.write_output(&pending.drain().text()).unwrap();

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

#[test]
fn push_transcript_item_with_open_output_executes_single_flush_transaction() {
    let (backend, operations, _) = FakeTerminal::new(40);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

    controller.write_stream_output("in-progress text").unwrap();
    operations.borrow_mut().clear();

    let notice = crate::ui::interactive::TranscriptItem::Notice("completed item".into());
    controller.push_transcript_item(notice).unwrap();

    let ops = operations.borrow();
    assert_eq!(ops.iter().filter(|op| op == &&Operation::Flush).count(), 1);
}

fn writes(operations: &super::fake::SharedOperations) -> Vec<String> {
    operations
        .borrow()
        .iter()
        .filter_map(|op| match op {
            Operation::Write(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_style_opened_in_one_streamed_write_is_restored_for_the_next_and_closed_before_the_repaint() {
    let (backend, operations, _) = FakeTerminal::new(40);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    controller.write_stream_output("\x1b[2mfirst \x1b[0m\x1b[2m ").unwrap();
    controller.write_stream_output("second\x1b[0m\n").unwrap();

    let written = writes(&operations);
    let first = written.iter().position(|w| w.starts_with("\x1b[2mfirst")).unwrap();
    assert_eq!(
        written[first + 1],
        "\x1b[0m",
        "the style is closed before the live region repaints"
    );
    let second = written.iter().position(|w| w.starts_with("second")).unwrap();
    assert_eq!(written[second - 1], "\x1b[2m", "the next write starts dim again");
}

#[test]
fn balanced_streamed_output_adds_no_style_writes() {
    let (backend, operations, _) = FakeTerminal::new(40);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    controller.write_stream_output("\x1b[2mone\x1b[0m ").unwrap();
    controller.write_stream_output("\x1b[1mtwo\x1b[0m").unwrap();

    let written = writes(&operations);
    let resets = written.iter().filter(|w| w.as_str() == "\x1b[0m").count();
    assert_eq!(resets, 0, "{written:?}");
}
