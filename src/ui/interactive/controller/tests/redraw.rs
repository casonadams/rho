use super::fake::{FakeTerminal, Operation};
use crate::ui::interactive::controller::TerminalController;
use crate::ui::interactive::{InteractiveState, ToolStartRequest};

#[test]
fn resize_erases_using_old_layout_and_redraws_at_new_width() {
    let (backend, operations, width) = FakeTerminal::new(8);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();
    width.set(4);

    assert!(controller.refresh_size().unwrap());

    let operations = operations.borrow();
    let clear_index = operations
        .iter()
        .position(|operation| operation == &Operation::Clear)
        .unwrap();
    let divider_index = operations
        .iter()
        .position(|operation| matches!(operation, Operation::Write(text) if text.contains("────")))
        .unwrap();
    assert!(clear_index < divider_index);
}

#[test]
fn resize_rerenders_at_new_width() {
    let (backend, operations, width) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    controller
        .start_tool(ToolStartRequest {
            name: "bash".into(),
            args_summary: "cargo test".into(),
            preview: None,
        })
        .unwrap();
    operations.borrow_mut().clear();
    width.set(30);

    assert!(controller.refresh_size().unwrap());

    let ops = operations.borrow();
    assert!(
        ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("Working...")))
    );
}

#[test]
fn tick_redraws_the_live_region() {
    let (backend, operations, _) = FakeTerminal::new(8);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    controller.tick().unwrap();

    let operations = operations.borrow();
    assert!(operations.contains(&Operation::Clear));
    assert!(
        operations
            .iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("────────")))
    );
    assert!(operations.ends_with(&[Operation::Show, Operation::Flush]));
}

#[test]
fn unchanged_size_does_not_redraw() {
    let (backend, operations, _) = FakeTerminal::new(8);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    assert!(!controller.refresh_size().unwrap());
    assert_eq!(*operations.borrow(), [Operation::Size]);
}

#[test]
fn resize_vertical_only_rerenders_and_updates_height() {
    let (backend, operations, _width, height) = FakeTerminal::with_size(60, 24);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    assert_eq!(controller.terminal_height(), 24);
    assert_eq!(controller.terminal_width(), 60);

    operations.borrow_mut().clear();
    height.set(12);

    assert!(controller.refresh_size().unwrap());
    assert_eq!(controller.terminal_height(), 12);
    assert_eq!(controller.terminal_width(), 60);

    let operations = operations.borrow();
    assert!(operations.contains(&Operation::Clear));
    assert!(operations.ends_with(&[Operation::Show, Operation::Flush]));
}

#[test]
fn resize_both_dimensions_rerenders_and_updates_both() {
    let (backend, operations, width, height) = FakeTerminal::with_size(60, 24);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

    operations.borrow_mut().clear();
    width.set(40);
    height.set(15);

    assert!(controller.refresh_size().unwrap());
    assert_eq!(controller.terminal_width(), 40);
    assert_eq!(controller.terminal_height(), 15);

    let ops = operations.borrow();
    assert!(ops.contains(&Operation::Clear));
}
