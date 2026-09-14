use super::common::HistoryTerminal;
use crate::repl::live::modal::{ModalKeyResult, handle_modal_key, open_remote_modal};
use crate::ui::interactive::{InteractiveState, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[test]
fn remote_modal_opens_and_navigates() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    open_remote_modal(&mut controller, "https://casonadams.github.io/rho/hub/#ticket=rho_abc");
    let modal = controller.state().active_modal().unwrap();
    assert_eq!(modal.title, "Remote Access");
    assert_eq!(modal.body, "https://casonadams.github.io/rho/hub/#ticket=rho_abc");
    assert_eq!(modal.options.len(), 3);

    let res = handle_modal_key(
        &mut controller,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &mut None,
    )
    .unwrap();
    assert_eq!(res, ModalKeyResult::Handled);
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn remote_modal_copy_shortcut() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    open_remote_modal(&mut controller, "https://casonadams.github.io/rho/hub/#ticket=rho_xyz");

    let res = handle_modal_key(
        &mut controller,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
        &mut None,
    )
    .unwrap();
    assert_eq!(res, ModalKeyResult::Handled);
    assert!(controller.state().active_modal().is_none());
    assert_eq!(
        controller.state().system_message(),
        Some("Copied pairing URL to clipboard")
    );
}
