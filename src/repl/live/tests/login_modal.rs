use super::common::HistoryTerminal;
use crate::repl::ReplSession;
use crate::repl::live::modal::{ModalKeyResult, handle_modal_key, open_login_selector};
use crate::ui::interactive::{InteractiveState, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_harness_core::config::Config;

fn send_modal_key(c: &mut TerminalController<HistoryTerminal>, code: KeyCode) -> ModalKeyResult {
    handle_modal_key(c, KeyEvent::new(code, KeyModifiers::NONE), &mut None).unwrap()
}

fn send_modal_char(c: &mut TerminalController<HistoryTerminal>, ch: char) -> ModalKeyResult {
    handle_modal_key(c, KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE), &mut None).unwrap()
}

fn setup_login_controller(provider: &str) -> (TerminalController<HistoryTerminal>, tempfile::TempDir) {
    let temp = tempfile::tempdir().unwrap();
    let auth_file = temp.path().join("auth.json");
    let mut auth_store = crate::auth::AuthStore::load(&auth_file).unwrap();
    auth_store.set_key("anthropic", "test-key").unwrap();
    let config = Config {
        provider: provider.to_string(),
        auth_file,
        ..Default::default()
    };
    let session = ReplSession::new(config, auth_store, None);
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    open_login_selector(&session, &mut controller);
    (controller, temp)
}

fn assert_contains_providers(modal: &crate::ui::interactive::ModalState) {
    let labels: std::collections::HashSet<_> = modal.options.iter().map(|o| o.label.trim()).collect();
    assert!(labels.contains("claude"));
    assert!(labels.contains("openai"));
    assert!(labels.contains("anthropic"));
    assert!(labels.contains("chatgpt"));
}

#[test]
fn login_selector_opens_with_clean_title_and_search() {
    let (controller, _temp) = setup_login_controller("claude");
    let modal = controller.state().active_modal().unwrap();
    assert_eq!(modal.title, "Login Provider");
    assert_eq!(modal.body, "");
    assert!(modal.is_searchable);
    assert_contains_providers(modal);
}

#[test]
fn login_selector_marks_configured_providers_with_check() {
    let (controller, _temp) = setup_login_controller("claude");
    let modal = controller.state().active_modal().unwrap();
    let anthropic_opt = modal.options.iter().find(|o| o.label.trim() == "anthropic").unwrap();
    assert!(anthropic_opt.description.as_deref().unwrap().contains('✓'));

    let openai_opt = modal.options.iter().find(|o| o.label.trim() == "openai").unwrap();
    assert!(!openai_opt.description.as_deref().unwrap().contains('✓'));
}

#[test]
fn login_selector_initial_selection_matches_active_provider() {
    let (controller, _temp) = setup_login_controller("claude");
    let modal = controller.state().active_modal().unwrap();
    let selected_opt = &modal.options[modal.selected];
    assert_eq!(selected_opt.label.trim(), "claude");
}

#[test]
fn login_selector_navigates_with_arrows() {
    let (mut controller, _temp) = setup_login_controller("antigravity");
    assert_eq!(controller.state().active_modal().unwrap().selected, 0);

    let _ = send_modal_key(&mut controller, KeyCode::Down);
    assert_eq!(controller.state().active_modal().unwrap().selected, 1);

    let _ = send_modal_key(&mut controller, KeyCode::Up);
    assert_eq!(controller.state().active_modal().unwrap().selected, 0);
}

#[test]
fn login_selector_filters_with_fuzzy_search() {
    let (mut controller, _temp) = setup_login_controller("antigravity");
    send_modal_char(&mut controller, 'g');
    send_modal_char(&mut controller, 'r');
    send_modal_char(&mut controller, 'o');
    send_modal_char(&mut controller, 'q');

    let modal = controller.state().active_modal().unwrap();
    assert_eq!(modal.options.len(), 1);
    assert_eq!(modal.options[0].label.trim(), "groq");

    send_modal_key(&mut controller, KeyCode::Backspace);
    let modal_back = controller.state().active_modal().unwrap();
    assert!(modal_back.options.len() > 1);
}

#[test]
fn login_selector_selects_on_enter() {
    let (mut controller, _temp) = setup_login_controller("claude");
    let res = send_modal_key(&mut controller, KeyCode::Enter);
    assert_eq!(
        res,
        ModalKeyResult::LoginProviderSelected {
            provider: "claude".to_string(),
        }
    );
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn login_selector_cancels_on_esc() {
    let (mut controller, _temp) = setup_login_controller("claude");
    let res = send_modal_key(&mut controller, KeyCode::Esc);
    assert_eq!(res, ModalKeyResult::Handled);
    assert!(controller.state().active_modal().is_none());
}
