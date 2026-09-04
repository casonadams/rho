use super::common::HistoryTerminal;
use crate::ui::interactive::{InteractiveState, TerminalController};

#[test]
fn session_selector_modal_selection() {
    let temp_dir = std::env::temp_dir().join(format!("test_sessions_{}", uuid::Uuid::new_v4()));
    let manager = rho_harness_core::session::SessionManager::new(&temp_dir, None).unwrap();
    let session_id = manager.session_id.clone();

    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_session_selector(&temp_dir, &mut controller);
    assert_eq!(controller.state().active_modal().unwrap().title, "Resume Session");

    let enter_key =
        crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
    let res = super::super::modal::handle_modal_key(&mut controller, enter_key, &mut None).unwrap();
    assert_eq!(res, super::super::modal::ModalKeyResult::SessionSelected { session_id });
    assert!(controller.state().active_modal().is_none());
    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn session_selector_modal_ctrl_d_deletes_session() {
    let temp_dir = std::env::temp_dir().join(format!("test_sessions_del_{}", uuid::Uuid::new_v4()));
    let manager = rho_harness_core::session::SessionManager::new(&temp_dir, None).unwrap();
    let session_id = manager.session_id.clone();

    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_session_selector(&temp_dir, &mut controller);

    let ctrl_d = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('d'),
        crossterm::event::KeyModifiers::CONTROL,
    );
    let res = super::super::modal::handle_modal_key(&mut controller, ctrl_d, &mut None).unwrap();
    assert_eq!(
        res,
        super::super::modal::ModalKeyResult::SessionDeleted {
            session_id: session_id.clone()
        }
    );
    assert!(controller.state().active_modal().unwrap().options.is_empty());
    let _ = std::fs::remove_dir_all(temp_dir);
}
