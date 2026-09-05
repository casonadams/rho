use super::common::HistoryTerminal;
use crate::ui::interactive::{InteractiveState, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_engine::provider::discovery::presets::anthropic_preset_models;
use rho_engine::provider::store::ModelStore;
use rho_harness_core::config::Config;
use rho_harness_core::state::AppState;

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

#[tokio::test]
async fn state_model_takes_precedence_over_saved_default_model() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_path_buf();

    // 1. Save default model to config.toml
    Config::save_default_model_async(&dir, "claude-3-7-sonnet-20250219", "anthropic")
        .await
        .unwrap();

    // 2. Simulate subsequent in-session model switch to a different model in state.json
    AppState::set_last_model_async(&dir, "gpt-4o", Some("openai"))
        .await
        .unwrap();

    // 3. Load config: must load the model from state.json, not the default model from config.toml
    unsafe {
        std::env::set_var("RHO_HOME", dir.to_str().unwrap());
    }
    let loaded = Config::load(None).unwrap();
    assert_eq!(
        loaded.model, "gpt-4o",
        "Config::load must use the model from state.json"
    );
    assert_eq!(loaded.provider, "openai");
    assert_eq!(loaded.default_model.as_deref(), Some("claude-3-7-sonnet-20250219"));
    assert_eq!(loaded.default_provider.as_deref(), Some("anthropic"));
    assert!(loaded.model_from_state);

    unsafe {
        std::env::remove_var("RHO_HOME");
    }
}
