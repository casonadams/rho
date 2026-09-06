use super::common::HistoryTerminal;
use crate::ui::interactive::{InteractiveState, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn send_modal_key(c: &mut TerminalController<HistoryTerminal>, code: KeyCode) -> super::super::modal::ModalKeyResult {
    super::super::modal::handle_modal_key(c, KeyEvent::new(code, KeyModifiers::NONE), &mut None).unwrap()
}

fn setup_model_selector_session(config_dir: std::path::PathBuf) -> crate::repl::ReplSession {
    let mut model_store = rho_engine::provider::store::ModelStore::load(config_dir.join("models-store.json"));
    model_store
        .set_models(
            "anthropic",
            rho_engine::provider::discovery::presets::anthropic_preset_models(),
        )
        .unwrap();
    let config = rho_harness_core::config::Config {
        config_dir,
        ..Default::default()
    };
    crate::repl::ReplSession::new(config, crate::auth::AuthStore::default(), None)
}

#[test]
fn model_selector_modal_filtering() {
    let temp = tempfile::tempdir().unwrap();
    let session = setup_model_selector_session(temp.path().to_path_buf());
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();

    super::super::modal::open_model_selector(&session, &mut controller);
    assert_eq!(controller.state().active_modal().unwrap().title, "Select Model");
    let key = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE);
    assert_eq!(
        super::super::modal::handle_modal_key(&mut controller, key, &mut None).unwrap(),
        super::super::modal::ModalKeyResult::Handled
    );
    assert_eq!(controller.state().active_modal().unwrap().filter_query, "s");
}

#[test]
fn model_selector_modal_selection() {
    let temp = tempfile::tempdir().unwrap();
    let session = setup_model_selector_session(temp.path().to_path_buf());
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();

    super::super::modal::open_model_selector(&session, &mut controller);
    controller.state_mut().active_modal_mut().unwrap().set_filter("claude");
    let res = send_modal_key(&mut controller, KeyCode::Enter);
    match res {
        super::super::modal::ModalKeyResult::ModelSelected {
            model,
            provider,
            save_as_default,
        } => {
            assert!(model.contains("claude") && !provider.is_empty() && !save_as_default);
        }
        _ => panic!("expected ModelSelected"),
    }
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn settings_selector_modal_toggles_hide_thinking() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    assert!(!controller.state().hide_thinking());
    super::super::modal::open_settings_selector(&mut controller);
    assert_eq!(
        send_modal_key(&mut controller, KeyCode::Enter),
        super::super::modal::ModalKeyResult::Handled
    );
    assert!(controller.state().hide_thinking());
    assert!(
        controller.state().active_modal().unwrap().options[0]
            .description
            .as_ref()
            .unwrap()
            .contains("Hidden")
    );
}

#[test]
fn settings_selector_modal_toggles_tools_expanded() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    assert!(!controller.state().tools_expanded());
    super::super::modal::open_settings_selector(&mut controller);
    let _ = send_modal_key(&mut controller, KeyCode::Down);
    assert_eq!(
        send_modal_key(&mut controller, KeyCode::Enter),
        super::super::modal::ModalKeyResult::Handled
    );
    assert!(controller.state().tools_expanded());
    assert!(
        controller.state().active_modal().unwrap().options[1]
            .description
            .as_ref()
            .unwrap()
            .contains("Expanded")
    );
}

fn open_test_theme_modal() -> TerminalController<HistoryTerminal> {
    let session = crate::repl::ReplSession::new(
        rho_harness_core::config::Config::default(),
        crate::auth::AuthStore::default(),
        None,
    );
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_theme_selector(&session, &mut controller);
    controller
}

#[test]
fn theme_selector_modal_navigation_and_selection() {
    let mut controller = open_test_theme_modal();
    let (title, count) = (
        controller.state().active_modal().unwrap().title.clone(),
        controller.state().active_modal().unwrap().options.len(),
    );
    assert_eq!((title.as_str(), count), ("Select Theme", 10));

    let _ = send_modal_key(&mut controller, KeyCode::Down);
    assert_eq!(
        (
            controller.state().active_modal().unwrap().selected,
            controller.theme().name.as_str()
        ),
        (1, "catppuccin")
    );

    let res = send_modal_key(&mut controller, KeyCode::Enter);
    assert_eq!(
        res,
        super::super::modal::ModalKeyResult::ThemeSelected {
            theme: "catppuccin".to_string()
        }
    );
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn theme_selector_modal_cancels_and_restores_original_theme() {
    let mut controller = open_test_theme_modal();
    assert_eq!(controller.theme().name, "default");
    let _ = send_modal_key(&mut controller, KeyCode::Down);
    assert_eq!(controller.theme().name, "catppuccin");

    let _ = send_modal_key(&mut controller, KeyCode::Esc);
    assert_eq!(
        (
            controller.theme().name.as_str(),
            controller.state().active_modal().is_none()
        ),
        ("default", true)
    );
}

fn release_key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: crossterm::event::KeyEventKind::Release,
        state: crossterm::event::KeyEventState::empty(),
    }
}

#[test]
fn modal_key_handler_ignores_key_release_events() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_settings_selector(&mut controller);
    assert_eq!(controller.state().active_modal().unwrap().selected, 0);

    let res = super::super::modal::handle_modal_key(&mut controller, release_key(KeyCode::Down), &mut None).unwrap();
    assert_eq!(
        (res, controller.state().active_modal().unwrap().selected),
        (super::super::modal::ModalKeyResult::Handled, 0)
    );

    let res = super::super::modal::handle_modal_key(&mut controller, release_key(KeyCode::Enter), &mut None).unwrap();
    assert_eq!(res, super::super::modal::ModalKeyResult::Handled);
    assert!(controller.state().active_modal().is_some() && !controller.state().hide_thinking());
}

async fn setup_model_switch_env(temp: &std::path::Path) -> (crate::repl::ReplSession, rho_engine::engine::AgentEngine) {
    let config = rho_harness_core::config::Config {
        model: "claude-3-5-sonnet-20241022".to_string(),
        provider: "anthropic".to_string(),
        config_dir: temp.to_path_buf(),
        ..Default::default()
    };
    let auth_store = crate::auth::AuthStore::default();
    let session = crate::repl::ReplSession::new(config.clone(), auth_store.clone(), None);
    let engine = rho_engine::engine::AgentEngineBuilder::new(config, auth_store)
        .build()
        .await
        .unwrap();
    (session, engine)
}

fn modal_test_env(
    temp: &std::path::Path,
) -> (
    TerminalController<HistoryTerminal>,
    crate::repl::interactive::InteractiveHistory,
    crate::repl::live::batch::LiveBatch,
) {
    (
        TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap(),
        crate::repl::interactive::InteractiveHistory::with_file(10, temp.join("history.txt")).unwrap(),
        crate::repl::live::batch::LiveBatch::new(),
    )
}

#[tokio::test]
async fn model_selector_selection_applies_model_switch_without_rebuild() {
    unsafe {
        std::env::set_var("ANTHROPIC_API_KEY", "test-key-not-real");
    }
    let temp = tempfile::tempdir().unwrap();
    let (mut session, mut engine) = setup_model_switch_env(temp.path()).await;
    let (mut controller, mut history, mut batch) = modal_test_env(temp.path());

    super::super::modal::open_model_selector(&session, &mut controller);
    let modal_res = send_modal_key(&mut controller, KeyCode::Enter);
    assert!(
        matches!(modal_res, super::super::modal::ModalKeyResult::ModelSelected { .. })
            && controller.state().active_modal().is_none()
    );

    let ctx = crate::repl::live::idle::modal_action::ModalActionContext {
        controller: &mut controller,
        history: &mut history,
        session: &mut session,
        engine: &mut engine,
    };
    assert!(
        crate::repl::live::idle::modal_action::apply_modal_key_result(modal_res, ctx, &mut batch)
            .await
            .unwrap()
    );
}
