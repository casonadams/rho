use super::common::HistoryTerminal;
use crate::auth::AuthStore;
use crate::repl::ReplSession;
use crate::repl::live::modal::{ModalKeyResult, handle_modal_key, open_model_selector};
use crate::repl::live::turn::{TurnModelSwitchInput, apply_turn_model_switch};
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{EditorState, FooterState, InteractiveState, LayoutInput, TerminalController, layout};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_engine::engine::runner::SharedModelSwitch;
use rho_engine::provider::discovery::claude_preset_models;
use rho_engine::provider::store::ModelStore;
use rho_harness_core::auth::StoredCredential;
use rho_harness_core::config::Config;

fn seed_claude_auth(auth_file: &std::path::Path) -> AuthStore {
    let mut auth_store = AuthStore::load(auth_file).unwrap();
    auth_store
        .set_credential(
            "claude",
            StoredCredential::OAuth {
                access_token: "test-access-token".into(),
                refresh_token: Some("test-refresh-token".into()),
                expires_at_ms: Some((chrono::Utc::now().timestamp() + 3600) * 1000),
                account_id: None,
                account_email: Some("user@example.com".into()),
            },
        )
        .unwrap();
    auth_store
}

fn setup_claude_session() -> (tempfile::TempDir, ReplSession) {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let auth_file = config_dir.join("auth.json");
    let auth_store = seed_claude_auth(&auth_file);

    let mut model_store = ModelStore::load(config_dir.join("models-store.json"));
    model_store.set_models("claude", claude_preset_models()).unwrap();

    let config = Config {
        config_dir,
        auth_file,
        model: "claude-sonnet-4-5".into(),
        provider: "claude".into(),
        ..Config::default()
    };
    (temp_dir, ReplSession::new(config, auth_store, None))
}

#[test]
fn model_selector_displays_claude_tag() {
    let (_dir, session) = setup_claude_session();
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    open_model_selector(&session, &mut controller);
    let modal = controller.state().active_modal().unwrap();
    assert_eq!(modal.title, "Select Model");

    let claude_opt = modal.options.iter().find(|o| o.label == "claude-sonnet-4-5").unwrap();
    assert!(claude_opt.description.as_deref().unwrap().starts_with("claude\t"));

    let rendered = layout(LayoutInput {
        editor: &EditorState::default(),
        modal: Some(modal),
        autocomplete: None,
        footer: &FooterState::default(),
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
    });
    assert!(rendered.editor_lines.iter().any(|l| l.contains("[claude]")));
}

#[test]
fn model_selector_selects_claude_model() {
    let (_dir, session) = setup_claude_session();
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    open_model_selector(&session, &mut controller);

    let res = handle_modal_key(
        &mut controller,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mut None,
    )
    .unwrap();
    match res {
        ModalKeyResult::ModelSelected {
            model,
            provider,
            save_as_default,
        } => {
            assert_eq!(
                (model.as_str(), provider.as_str(), save_as_default),
                ("claude-sonnet-4-5", "claude", false)
            );
        }
        _ => panic!("expected ModelSelected"),
    }
}

fn assert_model_switch_state(config: &Config, switch: &SharedModelSwitch, m: &str, p: &str) {
    assert_eq!((config.model.as_str(), config.provider.as_str()), (m, p));
    assert_eq!(
        (switch.current_model().as_deref(), switch.current_provider().as_deref()),
        (Some(m), Some(p))
    );
}

#[tokio::test]
async fn turn_model_switch_applies_claude_model_and_creates_handle() {
    let (_dir, session) = setup_claude_session();
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let mut batch = crate::repl::live::batch::LiveBatch::new();
    let mut config = session.config.clone();
    let renderer = TerminalRenderer::default();
    let model_switch = std::sync::Arc::new(SharedModelSwitch::new());

    let input = TurnModelSwitchInput {
        model: "claude-opus-4-6",
        provider: "claude",
        save_as_default: false,
        config: &mut config,
        auth_store: &session.auth_store,
        renderer: &renderer,
        controller: &mut controller,
        model_switch: &model_switch,
        batch: &mut batch,
        shared_auth: None,
    };

    apply_turn_model_switch(input).await.unwrap();
    assert_model_switch_state(&config, &model_switch, "claude-opus-4-6", "claude");
}
