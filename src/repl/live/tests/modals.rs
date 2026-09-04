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

#[test]
fn theme_selector_modal_navigation_and_selection() {
    let config = rho_harness_core::config::Config::default();
    let auth_store = crate::auth::AuthStore::load(&config.auth_file).unwrap_or_default();
    let session = crate::repl::ReplSession::new(config, auth_store, None);
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();

    super::super::modal::open_theme_selector(&session, &mut controller);
    assert_eq!(controller.state().active_modal().unwrap().title, "Select Theme");

    let modal = controller.state().active_modal().unwrap();
    assert_eq!(modal.options.len(), 10);
    assert_eq!(modal.options[0].label, "default");

    let down_key =
        crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Down, crossterm::event::KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, down_key, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().selected, 1);

    let enter_key =
        crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
    let res = super::super::modal::handle_modal_key(&mut controller, enter_key, &mut None).unwrap();

    match res {
        super::super::modal::ModalKeyResult::ThemeSelected { theme } => {
            assert_eq!(theme, "catppuccin");
        }
        _ => panic!("expected ThemeSelected result"),
    }
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn modal_key_handler_ignores_key_release_events() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_settings_selector(&mut controller);
    assert_eq!(controller.state().active_modal().unwrap().selected, 0);

    let release_down = crossterm::event::KeyEvent {
        code: crossterm::event::KeyCode::Down,
        modifiers: crossterm::event::KeyModifiers::NONE,
        kind: crossterm::event::KeyEventKind::Release,
        state: crossterm::event::KeyEventState::empty(),
    };
    let res = super::super::modal::handle_modal_key(&mut controller, release_down, &mut None).unwrap();
    assert_eq!(res, super::super::modal::ModalKeyResult::Handled);
    assert_eq!(controller.state().active_modal().unwrap().selected, 0);

    let release_enter = crossterm::event::KeyEvent {
        code: crossterm::event::KeyCode::Enter,
        modifiers: crossterm::event::KeyModifiers::NONE,
        kind: crossterm::event::KeyEventKind::Release,
        state: crossterm::event::KeyEventState::empty(),
    };
    let res = super::super::modal::handle_modal_key(&mut controller, release_enter, &mut None).unwrap();
    assert_eq!(res, super::super::modal::ModalKeyResult::Handled);
    assert!(controller.state().active_modal().is_some());
    assert!(!controller.state().hide_thinking());
}
