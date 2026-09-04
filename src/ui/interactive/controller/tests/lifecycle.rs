use super::fake::{FakeTerminal, Operation};
use crate::ui::interactive::InteractiveState;
use crate::ui::interactive::controller::TerminalController;

#[test]
fn construction_positions_and_shows_the_editor_cursor() {
    let (backend, operations, _) = FakeTerminal::new(10);

    let _controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

    let operations = operations.borrow();
    let show_index = operations
        .iter()
        .rposition(|operation| operation == &Operation::Show)
        .unwrap();
    let flush_index = operations
        .iter()
        .rposition(|operation| operation == &Operation::Flush)
        .unwrap();
    assert!(operations[..show_index].contains(&Operation::Hide));
    assert!(show_index < flush_index);
}

#[test]
fn construction_error_restores_cursor_and_raw_mode() {
    let (mut backend, operations, _) = FakeTerminal::new(8);
    backend.fail_write = true;

    assert!(TerminalController::new(backend, InteractiveState::default()).is_err());

    let operations = operations.borrow();
    assert!(operations.contains(&Operation::Show));
    assert!(operations.contains(&Operation::Raw(false)));
}

#[test]
fn suspend_and_resume_restore_terminal_modes_around_legacy_prompts() {
    let (backend, operations, _) = FakeTerminal::new(8);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    controller.suspend().unwrap();
    assert!(
        operations
            .borrow()
            .ends_with(&[Operation::Show, Operation::Raw(false), Operation::Flush])
    );
    operations.borrow_mut().clear();
    controller.resume().unwrap();
    assert_eq!(operations.borrow().first(), Some(&Operation::Raw(true)));
    assert!(operations.borrow().ends_with(&[Operation::Show, Operation::Flush]));
}

#[test]
fn drop_erases_region_and_restores_terminal() {
    let (backend, operations, _) = FakeTerminal::new(8);
    let controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    drop(controller);

    let operations = operations.borrow();
    assert!(operations.contains(&Operation::Clear));
    assert!(operations.ends_with(&[Operation::Show, Operation::Raw(false), Operation::Flush]));
}
