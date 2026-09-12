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
fn settings_selector_modal_toggles_block_style() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_settings_selector(None, None, &mut controller);
    assert_eq!(controller.state().active_modal().unwrap().selected, 0);

    let res = send_modal_key(&mut controller, KeyCode::Enter);
    assert_eq!(
        res,
        super::super::modal::ModalKeyResult::BlockStyleToggled {
            style: "border".to_string()
        }
    );
    assert_eq!(controller.block_style(), crate::ui::theme::BlockStyle::Border);
}

#[test]
fn settings_selector_modal_selects_model_opens_selector() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_settings_selector(Some("claude-3-7-sonnet"), Some("medium"), &mut controller);
    let key3 = KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, key3, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().selected, 2);
    assert_eq!(
        controller.state().active_modal().unwrap().options[2]
            .description
            .as_deref(),
        Some("claude-3-7-sonnet")
    );
    assert_eq!(
        send_modal_key(&mut controller, KeyCode::Enter),
        super::super::modal::ModalKeyResult::OpenModelSelector
    );
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn settings_selector_modal_cycles_thinking_effort() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_settings_selector(Some("claude-3-7-sonnet"), Some("medium"), &mut controller);
    let key4 = KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, key4, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().selected, 3);
    assert_eq!(
        controller.state().active_modal().unwrap().options[3]
            .description
            .as_deref(),
        Some("medium")
    );
    assert_eq!(
        send_modal_key(&mut controller, KeyCode::Enter),
        super::super::modal::ModalKeyResult::ThinkingLevelSelected {
            level: Some("high".to_string()),
            save_as_default: false,
        }
    );
    assert_eq!(
        controller.state().active_modal().unwrap().options[3]
            .description
            .as_deref(),
        Some("high")
    );
}

#[test]
fn settings_selector_modal_toggles_thinking_output() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    assert!(!controller.state().hide_thinking());
    super::super::modal::open_settings_selector(None, None, &mut controller);
    let key5 = KeyEvent::new(KeyCode::Char('5'), KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, key5, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().selected, 4);
    assert_eq!(
        send_modal_key(&mut controller, KeyCode::Enter),
        super::super::modal::ModalKeyResult::Handled
    );
    assert!(controller.state().hide_thinking());
    assert!(
        controller.state().active_modal().unwrap().options[4]
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
    super::super::modal::open_settings_selector(None, None, &mut controller);
    let key6 = KeyEvent::new(KeyCode::Char('6'), KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, key6, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().selected, 5);
    assert_eq!(
        send_modal_key(&mut controller, KeyCode::Enter),
        super::super::modal::ModalKeyResult::Handled
    );
    assert!(controller.state().tools_expanded());
    assert!(
        controller.state().active_modal().unwrap().options[5]
            .description
            .as_ref()
            .unwrap()
            .contains("Expanded")
    );
}

#[test]
fn settings_selector_modal_toggles_box_responses_and_labels() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_settings_selector(None, None, &mut controller);

    // Jump to Box Responses (digit 2 -> index 1)
    let key2 = KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, key2, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().selected, 1);

    let res = send_modal_key(&mut controller, KeyCode::Enter);
    assert_eq!(
        res,
        super::super::modal::ModalKeyResult::AgentBoxToggled { boxed: true }
    );
    assert!(controller.block_agent_output());

    // Jump to Version Banner (digit 7 -> index 6)
    let key7 = KeyEvent::new(KeyCode::Char('7'), KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, key7, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().selected, 6);

    let res = send_modal_key(&mut controller, KeyCode::Enter);
    assert_eq!(
        res,
        super::super::modal::ModalKeyResult::ShowLabelToggled { shown: true }
    );
    assert!(controller.state().show_label());
}

#[test]
fn settings_selector_modal_arrows_step_thinking_effort() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_settings_selector(None, Some("medium"), &mut controller);
    let key4 = KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, key4, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().selected, 3);

    // Left arrow goes backward (medium -> low)
    let res = super::super::modal::handle_modal_key(
        &mut controller,
        KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
        &mut None,
    )
    .unwrap();
    assert_eq!(
        res,
        super::super::modal::ModalKeyResult::ThinkingLevelSelected {
            level: Some("low".to_string()),
            save_as_default: false,
        }
    );
    assert_eq!(
        controller.state().active_modal().unwrap().options[3]
            .description
            .as_deref(),
        Some("low")
    );

    // Right arrow goes forward (low -> medium)
    let res = super::super::modal::handle_modal_key(
        &mut controller,
        KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
        &mut None,
    )
    .unwrap();
    assert_eq!(
        res,
        super::super::modal::ModalKeyResult::ThinkingLevelSelected {
            level: Some("medium".to_string()),
            save_as_default: false,
        }
    );
}

#[test]
fn settings_selector_modal_ctrl_s_saves_thinking_as_default() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_settings_selector(None, Some("high"), &mut controller);
    let key4 = KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, key4, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().selected, 3);

    let ctrl_s = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
    let res = super::super::modal::handle_modal_key(&mut controller, ctrl_s, &mut None).unwrap();
    assert_eq!(
        res,
        super::super::modal::ModalKeyResult::ThinkingLevelSelected {
            level: Some("high".to_string()),
            save_as_default: true,
        }
    );
}

#[test]
fn settings_selector_modal_digit_jump_navigates() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_settings_selector(None, None, &mut controller);
    assert_eq!(controller.state().active_modal().unwrap().selected, 0);

    // Press '6' jumps to Tool Output (index 5)
    let key6 = KeyEvent::new(KeyCode::Char('6'), KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, key6, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().selected, 5);

    // Press '1' jumps back to Agent Box Output (index 0)
    let key1 = KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, key1, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().selected, 0);
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
    super::super::modal::open_settings_selector(None, None, &mut controller);
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
