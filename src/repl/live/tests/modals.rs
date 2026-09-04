use super::common::HistoryTerminal;
use crate::ui::interactive::{InteractiveState, TerminalController};

#[test]
fn model_selector_modal_filtering_and_selection() {
    let config = rho_harness_core::config::Config::default();
    let auth_store = crate::auth::AuthStore::load(&config.auth_file).unwrap_or_default();
    let session = crate::repl::ReplSession::new(config, auth_store, None);
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();

    super::super::modal::open_model_selector(&session, &mut controller);
    assert_eq!(controller.state().active_modal().unwrap().title, "Select Model");

    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('s'),
        crossterm::event::KeyModifiers::NONE,
    );
    let res = super::super::modal::handle_modal_key(&mut controller, key, &mut None).unwrap();
    assert_eq!(res, super::super::modal::ModalKeyResult::Handled);
    assert_eq!(controller.state().active_modal().unwrap().filter_query, "s");

    if let Some(modal) = controller.state_mut().active_modal_mut() {
        modal.set_filter("claude");
    }
    let modal = controller.state().active_modal().unwrap();
    assert!(modal.options.iter().any(|o| o.label.contains("claude")));

    let enter_key =
        crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
    let res = super::super::modal::handle_modal_key(&mut controller, enter_key, &mut None).unwrap();
    match res {
        super::super::modal::ModalKeyResult::ModelSelected {
            model,
            provider,
            save_as_default,
        } => {
            assert!(model.contains("claude"));
            assert!(!provider.is_empty());
            assert!(!save_as_default);
        }
        _ => panic!("expected ModelSelected result"),
    }
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn settings_selector_modal_toggles() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    assert!(!controller.state().hide_thinking());
    assert!(!controller.state().tools_expanded());

    super::super::modal::open_settings_selector(&mut controller);
    assert_eq!(controller.state().active_modal().unwrap().title, "Settings");

    let enter_key =
        crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
    let res = super::super::modal::handle_modal_key(&mut controller, enter_key, &mut None).unwrap();
    assert_eq!(res, super::super::modal::ModalKeyResult::Handled);
    assert!(controller.state().hide_thinking());
    assert!(
        controller.state().active_modal().unwrap().options[0]
            .description
            .as_ref()
            .unwrap()
            .contains("Hidden")
    );

    let down_key =
        crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Down, crossterm::event::KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, down_key, &mut None).unwrap();
    let res = super::super::modal::handle_modal_key(&mut controller, enter_key, &mut None).unwrap();
    assert_eq!(res, super::super::modal::ModalKeyResult::Handled);
    assert!(controller.state().tools_expanded());
    assert!(
        controller.state().active_modal().unwrap().options[1]
            .description
            .as_ref()
            .unwrap()
            .contains("Expanded")
    );
}
