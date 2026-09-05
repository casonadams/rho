use super::common::HistoryTerminal;
use crate::ui::interactive::{InteractiveState, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_engine::provider::discovery::presets::anthropic_preset_models;
use rho_engine::provider::store::ModelStore;
use rho_harness_core::config::Config;

#[test]
fn model_selector_marks_default_model_separately_from_active() {
    let temp = tempfile::tempdir().unwrap();
    let config_dir = temp.path().to_path_buf();
    let mut model_store = ModelStore::load(config_dir.join("models-store.json"));
    model_store.set_models("anthropic", anthropic_preset_models()).unwrap();

    let config = Config {
        config_dir: config_dir.clone(),
        model: "claude-3-5-haiku-20241022".into(),
        provider: "anthropic".into(),
        default_model: Some("claude-3-7-sonnet-20250219".into()),
        default_provider: Some("anthropic".into()),
        ..Config::default()
    };
    let auth_store = crate::auth::AuthStore::default();
    let session = crate::repl::ReplSession::new(config, auth_store, None);
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();

    super::super::modal::open_model_selector(&session, &mut controller);
    let modal = controller.state().active_modal().unwrap();

    let active_opt = modal
        .options
        .iter()
        .find(|o| o.label == "claude-3-5-haiku-20241022")
        .expect("active model option exists");
    let active_desc = active_opt.description.as_deref().unwrap();
    let mut active_parts = active_desc.split('\t');
    let _prov = active_parts.next();
    let active_mark = active_parts.next().unwrap();
    let default_mark = active_parts.next().unwrap();
    assert_eq!(active_mark, "✓", "active model must have checkmark");
    assert_eq!(default_mark, "", "active model must not be falsely marked default");

    let default_opt = modal
        .options
        .iter()
        .find(|o| o.label == "claude-3-7-sonnet-20250219")
        .expect("default model option exists");
    let default_desc = default_opt.description.as_deref().unwrap();
    let mut default_parts = default_desc.split('\t');
    let _prov = default_parts.next();
    let active_mark = default_parts.next().unwrap();
    let default_mark = default_parts.next().unwrap();
    assert_eq!(active_mark, "", "inactive default model must not have checkmark");
    assert_eq!(default_mark, "default", "saved default model must have default mark");
}

#[test]
fn ctrl_s_key_saves_selected_model_as_default() {
    let config = Config::default();
    let auth_store = crate::auth::AuthStore::load(&config.auth_file).unwrap_or_default();
    let session = crate::repl::ReplSession::new(config, auth_store, None);
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();

    super::super::modal::open_model_selector(&session, &mut controller);

    let ctrl_s = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
    let res = super::super::modal::handle_modal_key(&mut controller, ctrl_s, &mut None).unwrap();

    match res {
        super::super::modal::ModalKeyResult::ModelSelected {
            model: _,
            provider: _,
            save_as_default,
        } => {
            assert!(save_as_default, "Ctrl+S must trigger save_as_default");
        }
        _ => panic!("expected ModelSelected result"),
    }
}
