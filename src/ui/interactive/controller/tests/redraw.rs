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
