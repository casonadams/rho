use super::fake::{FakeTerminal, Operation};
use crate::ui::interactive::controller::TerminalController;
use crate::ui::interactive::{InteractiveState, TranscriptItem};

#[test]
fn full_redraw_rerenders_all_transcript_items_on_resize() {
    let (backend, operations, width) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    controller
        .push_transcript_item(TranscriptItem::UserMessage("hello world message".into()))
        .unwrap();
    operations.borrow_mut().clear();

    width.set(40);
    assert!(controller.refresh_size().unwrap());

    let ops = operations.borrow();
    assert!(ops.contains(&Operation::Write("\x1b[2J\x1b[H\x1b[3J".into())));
    assert!(
        ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("hello world message")))
    );
}

#[test]
fn assistant_transcript_item_is_recorded_without_duplicate_write_output() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    controller
        .push_transcript_item(TranscriptItem::AssistantText("streamed response answer".into()))
        .unwrap();

    assert_eq!(controller.transcript().len(), 1);
    let ops = operations.borrow();
    assert!(
        !ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("streamed response answer"))),
        "pushing already-streamed assistant text should not write to output again"
    );
}
