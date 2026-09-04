use super::fake::{FakeTerminal, Operation};
use crate::ui::interactive::controller::TerminalController;
use crate::ui::interactive::{Activity, InteractiveState};

#[test]
fn busy_working_line_renders_above_the_editor() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut state = InteractiveState::default();
    state.footer_mut().activity = Activity::Thinking;
    let mut controller = TerminalController::new(backend, state).unwrap();
    operations.borrow_mut().clear();

    controller.tick().unwrap();

    let ops = operations.borrow();
    let working_index = ops.iter().position(|op| {
        matches!(
            op,
            Operation::Write(text) if text.contains("Thinking...") || text.contains("Working...")
        )
    });
    let divider_index = ops
        .iter()
        .position(|op| matches!(op, Operation::Write(text) if text.contains(&"\u{2500}".repeat(60))));

    assert!(working_index.is_some());
    assert!(working_index.unwrap() < divider_index.unwrap());
}

#[test]
fn busy_working_line_disappears_when_idle() {
    let (backend, operations, _) = FakeTerminal::new(20);
    let mut state = InteractiveState::default();
    state.footer_mut().activity = Activity::Working;
    let mut controller = TerminalController::new(backend, state).unwrap();
    controller.state_mut().footer_mut().activity = Activity::Idle;
    operations.borrow_mut().clear();

    controller.tick().unwrap();

    let ops = operations.borrow();
    assert!(
        !ops.iter().any(
            |op| matches!(op, Operation::Write(text) if text.contains("Working...") || text.contains("Thinking..."))
        )
    );
}

#[test]
fn footer_carries_no_spinner_or_activity_label_when_busy() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut state = InteractiveState::default();
    state.footer_mut().activity = Activity::Working;
    state.footer_mut().model = "model".into();
    let mut controller = TerminalController::new(backend, state).unwrap();
    operations.borrow_mut().clear();

    controller.tick().unwrap();

    let ops = operations.borrow();
    assert!(
        ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("model") && text.contains("\u{1b}[2m")))
    );
    assert!(
        !ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("working")))
    );
    assert!(
        !ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("thinking")))
    );
}

#[test]
fn idle_footer_is_rendered_dimmed() {
    let (backend, operations, _) = FakeTerminal::new(20);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    controller.tick().unwrap();

    assert!(
        operations
            .borrow()
            .iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("\u{1b}[2m")))
    );
}
