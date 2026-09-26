use super::common::HistoryTerminal;
use crate::auth::AuthStore;
use crate::repl::ReplSession;
use crate::repl::live::modal::{
    ModalKeyResult, handle_modal_key, open_help_selector, open_login_selector, open_mcp_selector, open_model_selector,
    open_session_selector, open_tree_selector,
};
use crate::repl::live::turn::{TurnModelSwitchInput, apply_turn_model_switch};
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{EditorState, FooterState, InteractiveState, LayoutInput, TerminalController, layout};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_engine::engine::runner::SharedModelSwitch;
use rho_engine::provider::discovery::claude_preset_models;
use rho_engine::provider::store::ModelStore;
use rho_harness_core::auth::StoredCredential;
use rho_harness_core::config::{Config, McpConfig, McpServerConfig};

fn send_modal_key(c: &mut TerminalController<HistoryTerminal>, code: KeyCode) -> ModalKeyResult {
    handle_modal_key(c, KeyEvent::new(code, KeyModifiers::NONE), &mut None).unwrap()
}

fn send_modal_char(c: &mut TerminalController<HistoryTerminal>, ch: char) -> ModalKeyResult {
    handle_modal_key(c, KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE), &mut None).unwrap()
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
    super::super::modal::open_settings_selector(None, None, None, false, &mut controller);
    assert_eq!(controller.state().active_modal().unwrap().selected, 0);

    let res = send_modal_key(&mut controller, KeyCode::Enter);
    assert_eq!(
        res,
        super::super::modal::ModalKeyResult::BlockStyleToggled {
            style: "solid".to_string()
        }
    );
    assert_eq!(controller.block_style(), crate::ui::theme::BlockStyle::Solid);
}

#[test]
fn settings_selector_modal_opens_tools_menu() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_settings_selector(None, None, None, false, &mut controller);

    let modal = controller.state_mut().active_modal_mut().unwrap();
    modal.selected = 10;

    let res = send_modal_key(&mut controller, KeyCode::Enter);
    assert_eq!(res, super::super::modal::ModalKeyResult::OpenToolsMenu);
}

#[test]
fn settings_selector_modal_toggles_cursor_mode() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_settings_selector(None, None, None, false, &mut controller);
    let key9 = KeyEvent::new(KeyCode::Char('9'), KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, key9, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().selected, 8);

    let res = send_modal_key(&mut controller, KeyCode::Enter);
    assert_eq!(
        res,
        super::super::modal::ModalKeyResult::CursorToggled {
            cursor: "software".to_string()
        }
    );
    assert_eq!(controller.cursor_mode(), crate::ui::theme::CursorMode::Software);
}

#[test]
fn settings_selector_modal_toggles_semantic_search() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_settings_selector(None, None, None, false, &mut controller);
    let modal = controller.state_mut().active_modal_mut().unwrap();
    modal.selected = 9;

    let res = send_modal_key(&mut controller, KeyCode::Enter);
    assert_eq!(
        res,
        super::super::modal::ModalKeyResult::SemanticSearchToggled { enabled: true }
    );
    assert_eq!(
        controller.state().active_modal().unwrap().options[9]
            .description
            .as_deref(),
        Some("On")
    );
}

#[test]
fn settings_selector_modal_selects_model_opens_selector() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_settings_selector(
        Some("claude-3-7-sonnet"),
        None,
        Some("medium"),
        false,
        &mut controller,
    );
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
        super::super::modal::ModalKeyResult::OpenModelSelector { save_as_default: true }
    );
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn settings_selector_modal_selects_guard_model_opens_selector() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_settings_selector(
        Some("claude-3-7-sonnet"),
        Some("local/qwen2.5-coder:7b"),
        Some("medium"),
        false,
        &mut controller,
    );
    let key4 = KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, key4, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().selected, 3);
    assert_eq!(
        controller.state().active_modal().unwrap().options[3]
            .description
            .as_deref(),
        Some("local/qwen2.5-coder:7b")
    );
    assert_eq!(
        send_modal_key(&mut controller, KeyCode::Enter),
        super::super::modal::ModalKeyResult::OpenGuardModelSelector
    );
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn settings_selector_modal_cycles_thinking_effort() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_settings_selector(
        Some("claude-3-7-sonnet"),
        None,
        Some("medium"),
        false,
        &mut controller,
    );
    let key5 = KeyEvent::new(KeyCode::Char('5'), KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, key5, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().selected, 4);
    assert_eq!(
        controller.state().active_modal().unwrap().options[4]
            .description
            .as_deref(),
        Some("medium")
    );
    assert_eq!(
        send_modal_key(&mut controller, KeyCode::Enter),
        super::super::modal::ModalKeyResult::ThinkingLevelSelected {
            level: Some("high".to_string()),
            save_as_default: true,
        }
    );
    assert_eq!(
        controller.state().active_modal().unwrap().options[4]
            .description
            .as_deref(),
        Some("high")
    );
}

#[test]
fn settings_selector_modal_toggles_thinking_output() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    assert!(!controller.state().hide_thinking());
    super::super::modal::open_settings_selector(None, None, None, false, &mut controller);
    let key6 = KeyEvent::new(KeyCode::Char('6'), KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, key6, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().selected, 5);
    assert_eq!(
        send_modal_key(&mut controller, KeyCode::Enter),
        super::super::modal::ModalKeyResult::ThinkingOutputToggled { hidden: true }
    );
    assert!(controller.state().hide_thinking());
    assert!(
        controller.state().active_modal().unwrap().options[5]
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
    super::super::modal::open_settings_selector(None, None, None, false, &mut controller);
    let key7 = KeyEvent::new(KeyCode::Char('7'), KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, key7, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().selected, 6);
    assert_eq!(
        send_modal_key(&mut controller, KeyCode::Enter),
        super::super::modal::ModalKeyResult::ToolOutputToggled { expanded: true }
    );
    assert!(controller.state().tools_expanded());
    assert!(
        controller.state().active_modal().unwrap().options[6]
            .description
            .as_ref()
            .unwrap()
            .contains("Expanded")
    );
}

#[test]
fn settings_selector_modal_toggles_box_responses_and_labels() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_settings_selector(None, None, None, false, &mut controller);

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

    // Jump to Version Banner (digit 8 -> index 7)
    let key8 = KeyEvent::new(KeyCode::Char('8'), KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, key8, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().selected, 7);

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
    super::super::modal::open_settings_selector(None, None, Some("medium"), false, &mut controller);
    let key5 = KeyEvent::new(KeyCode::Char('5'), KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, key5, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().selected, 4);

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
            save_as_default: true,
        }
    );
    assert_eq!(
        controller.state().active_modal().unwrap().options[4]
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
            save_as_default: true,
        }
    );
}

#[test]
fn settings_selector_modal_ctrl_s_saves_thinking_as_default() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_settings_selector(None, None, Some("high"), false, &mut controller);
    let key5 = KeyEvent::new(KeyCode::Char('5'), KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, key5, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().selected, 4);

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
    super::super::modal::open_settings_selector(None, None, None, false, &mut controller);
    assert_eq!(controller.state().active_modal().unwrap().selected, 0);

    // Press '7' jumps to Tool Output (index 6)
    let key7 = KeyEvent::new(KeyCode::Char('7'), KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, key7, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().selected, 6);

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
    super::super::modal::open_settings_selector(None, None, None, false, &mut controller);
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
    let mut auth_store = crate::auth::AuthStore::default();
    let _ = auth_store.set_api_key("anthropic", "test-anthropic-key");
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

    let mut input = crate::repl::input_reader::TerminalInputReader::spawn_dummy();
    let ctx = crate::repl::live::idle::modal_action::ModalActionContext {
        controller: &mut controller,
        history: &mut history,
        session: &mut session,
        engine: &mut engine,
        input: &mut input,
    };
    assert!(
        crate::repl::live::idle::modal_action::apply_modal_key_result(modal_res, ctx, &mut batch)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn model_selector_modal_with_save_as_default_emits_true_on_enter() {
    let temp = tempfile::tempdir().unwrap();
    let (session, _) = setup_model_switch_env(temp.path()).await;
    let (mut controller, _, _) = modal_test_env(temp.path());

    super::super::modal::open_model_selector_with_default(&session, &mut controller, true);
    let modal_res = send_modal_key(&mut controller, KeyCode::Enter);
    match modal_res {
        super::super::modal::ModalKeyResult::ModelSelected { save_as_default, .. } => assert!(save_as_default),
        other => panic!("expected ModelSelected, got {other:?}"),
    }
}

#[tokio::test]
async fn settings_modal_actions_persist_to_disk() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, mut engine) = setup_model_switch_env(temp.path()).await;
    let (mut controller, mut history, mut batch) = modal_test_env(temp.path());

    let actions = vec![
        super::super::modal::ModalKeyResult::BlockStyleToggled {
            style: "solid".to_string(),
        },
        super::super::modal::ModalKeyResult::AgentBoxToggled { boxed: true },
        super::super::modal::ModalKeyResult::ShowLabelToggled { shown: true },
        super::super::modal::ModalKeyResult::ThinkingOutputToggled { hidden: true },
        super::super::modal::ModalKeyResult::ToolOutputToggled { expanded: true },
        super::super::modal::ModalKeyResult::CursorToggled {
            cursor: "hardware".to_string(),
        },
        super::super::modal::ModalKeyResult::ThinkingLevelSelected {
            level: Some("high".to_string()),
            save_as_default: true,
        },
        super::super::modal::ModalKeyResult::ModelSelected {
            model: "claude-3-5-haiku-20241022".to_string(),
            provider: "anthropic".to_string(),
            save_as_default: true,
        },
    ];

    let mut input = crate::repl::input_reader::TerminalInputReader::spawn_dummy();
    for action in actions {
        let ctx = crate::repl::live::idle::modal_action::ModalActionContext {
            controller: &mut controller,
            history: &mut history,
            session: &mut session,
            engine: &mut engine,
            input: &mut input,
        };
        assert!(
            crate::repl::live::idle::modal_action::apply_modal_key_result(action, ctx, &mut batch)
                .await
                .unwrap()
        );
    }

    let config_path = temp.path().join("config.toml");
    let content = std::fs::read_to_string(&config_path).expect("config.toml written");
    let toml: toml::Value = toml::from_str(&content).unwrap();

    assert_eq!(
        toml.get("model").and_then(|v| v.as_str()),
        Some("claude-3-5-haiku-20241022")
    );
    assert_eq!(toml.get("provider").and_then(|v| v.as_str()), Some("anthropic"));
    assert_eq!(toml.get("thinking_level").and_then(|v| v.as_str()), Some("high"));
    assert_eq!(toml.get("show_label").and_then(|v| v.as_bool()), Some(true));

    let ui = toml.get("ui").expect("ui section in config");
    assert_eq!(ui.get("block_style").and_then(|v| v.as_str()), Some("solid"));
    assert_eq!(ui.get("agent_block_output").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(ui.get("hide_thinking").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(ui.get("tools_expanded").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(ui.get("cursor").and_then(|v| v.as_str()), Some("hardware"));
}

#[tokio::test]
async fn init_live_state_hydrates_ui_preferences() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, engine) = setup_model_switch_env(temp.path()).await;
    session.config.ui.hide_thinking = Some(true);
    session.config.ui.tools_expanded = Some(true);
    session.config.show_label = true;

    let state = super::super::setup::init_live_state(&session, &engine);
    assert!(state.hide_thinking());
    assert!(state.tools_expanded());
    assert!(state.show_label());

    let controller = TerminalController::new(HistoryTerminal, state).unwrap();
    assert!(controller.hide_thinking());
    assert!(controller.tools_expanded());
    assert!(controller.state().show_label());
}

// =========================================================================
// Help Modal Tests
// =========================================================================

#[test]
fn help_modal_opens_with_commands_and_shortcuts() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    open_help_selector(&mut controller);
    let modal = controller.state().active_modal().unwrap();
    assert_eq!(modal.title, "Help");
    assert_eq!(modal.body, "");
    assert!(modal.is_searchable);

    let labels: Vec<&str> = modal.options.iter().map(|o| o.label.trim()).collect();
    let expected_items = [
        "/settings",
        "/model",
        "/resume",
        "/session",
        "/compact",
        "/tree",
        "/mcp",
        "/login",
        "/clear",
        "Tab",
        "Shift+Tab",
        "Ctrl+L",
        "Escape",
    ];
    for expected in expected_items {
        assert!(labels.contains(&expected));
    }
}

#[test]
fn help_modal_navigates_and_filters() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    open_help_selector(&mut controller);
    assert_eq!(controller.state().active_modal().unwrap().selected, 0);

    let _ = send_modal_key(&mut controller, KeyCode::Down);
    assert_eq!(controller.state().active_modal().unwrap().selected, 1);

    let _ = send_modal_key(&mut controller, KeyCode::Up);
    assert_eq!(controller.state().active_modal().unwrap().selected, 0);

    send_modal_char(&mut controller, 'm');
    send_modal_char(&mut controller, 'c');
    send_modal_char(&mut controller, 'p');

    let modal = controller.state().active_modal().unwrap();
    assert!(!modal.options.is_empty());
    assert_eq!(modal.options[0].label.trim(), "/mcp");

    send_modal_key(&mut controller, KeyCode::Backspace);
    let modal_back = controller.state().active_modal().unwrap();
    assert!(modal_back.options.len() > 1);
}

#[test]
fn help_modal_enter_on_command_returns_command_selected() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    open_help_selector(&mut controller);

    let res = send_modal_key(&mut controller, KeyCode::Enter);
    assert_eq!(
        res,
        ModalKeyResult::HelpCommandSelected {
            command: "/settings".to_string()
        }
    );
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn help_modal_esc_and_ctrl_c_dismiss() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    open_help_selector(&mut controller);
    assert!(controller.state().active_modal().is_some());

    let res = send_modal_key(&mut controller, KeyCode::Esc);
    assert_eq!(res, ModalKeyResult::Handled);
    assert!(controller.state().active_modal().is_none());

    open_help_selector(&mut controller);
    send_modal_char(&mut controller, 'x');
    assert_eq!(controller.state().active_modal().unwrap().filter_query, "x");

    let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    let res = handle_modal_key(&mut controller, ctrl_c, &mut None).unwrap();
    assert_eq!(res, ModalKeyResult::Handled);
    assert_eq!(controller.state().active_modal().unwrap().filter_query, "");
    assert!(controller.state().active_modal().is_some());

    let res = handle_modal_key(&mut controller, ctrl_c, &mut None).unwrap();
    assert_eq!(res, ModalKeyResult::Handled);
    assert!(controller.state().active_modal().is_none());
}

// =========================================================================
// Login Modal Tests
// =========================================================================

fn setup_login_controller(provider: &str) -> (TerminalController<HistoryTerminal>, tempfile::TempDir) {
    let temp = tempfile::tempdir().unwrap();
    let auth_file = temp.path().join("auth.json");
    let mut auth_store = AuthStore::load(&auth_file).unwrap();
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

#[tokio::test]
async fn login_provider_selected_dispatches_with_input_pause_and_resume() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, mut engine) = setup_model_switch_env(temp.path()).await;
    let (mut controller, mut history, mut batch) = modal_test_env(temp.path());
    let mut input = crate::repl::input_reader::TerminalInputReader::spawn_dummy();

    let ctx = crate::repl::live::idle::modal_action::ModalActionContext {
        controller: &mut controller,
        history: &mut history,
        session: &mut session,
        engine: &mut engine,
        input: &mut input,
    };

    let action = ModalKeyResult::LoginProviderSelected {
        provider: "local".to_string(),
    };
    let handled = crate::repl::live::idle::modal_action::apply_modal_key_result(action, ctx, &mut batch)
        .await
        .unwrap();
    assert!(handled);
}

// =========================================================================
// MCP Modal Tests
// =========================================================================

fn setup_mcp_controller() -> TerminalController<HistoryTerminal> {
    let mut servers = std::collections::BTreeMap::new();
    servers.insert(
        "filesystem".to_string(),
        McpServerConfig::stdio("npx", vec!["-y".to_string()]),
    );
    let remote = McpServerConfig {
        url: Some("https://example.com/mcp".to_string()),
        enabled: false,
        ..Default::default()
    };
    servers.insert("remote_tool".to_string(), remote);

    let config = Config {
        mcp: McpConfig {
            enabled: true,
            defer_threshold: 10,
            servers,
        },
        ..Default::default()
    };
    let session = ReplSession::new(config, AuthStore::default(), None);
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    open_mcp_selector(&session, &mut controller);
    controller
}

#[test]
fn mcp_selector_opens_with_configured_servers() {
    let controller = setup_mcp_controller();
    let modal = controller.state().active_modal().unwrap();
    assert_eq!(modal.title, "Model Context Protocol");
    assert_eq!(modal.options.len(), 2);
}

#[test]
fn mcp_selector_marks_active_servers() {
    let controller = setup_mcp_controller();
    let modal = controller.state().active_modal().unwrap();
    assert!(modal.options[0].description.as_deref().unwrap().contains('✓'));
    assert!(modal.options[1].description.as_deref().unwrap().contains("(off)"));
}

#[test]
fn mcp_selector_navigates_and_toggles() {
    let mut controller = setup_mcp_controller();
    let _ = send_modal_key(&mut controller, KeyCode::Down);
    assert_eq!(controller.state().active_modal().unwrap().selected, 1);

    let res = send_modal_key(&mut controller, KeyCode::Enter);
    assert_eq!(
        res,
        ModalKeyResult::McpServerToggled {
            server: "remote_tool".to_string()
        }
    );
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn mcp_selector_cancels_on_esc() {
    let mut controller = setup_mcp_controller();
    let res = send_modal_key(&mut controller, KeyCode::Esc);
    assert_eq!(res, ModalKeyResult::Handled);
    assert!(controller.state().active_modal().is_none());
}

// =========================================================================
// Session Modal Tests
// =========================================================================

#[test]
fn session_selector_modal_selection() {
    let temp_dir = std::env::temp_dir().join(format!("test_sessions_{}", uuid::Uuid::new_v4()));
    let manager = rho_harness_core::session::SessionManager::new(&temp_dir, None).unwrap();
    let session_id = manager.session_id.clone();

    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    open_session_selector(&temp_dir, &mut controller);
    assert_eq!(controller.state().active_modal().unwrap().title, "Resume Session");

    let enter_key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let res = handle_modal_key(&mut controller, enter_key, &mut None).unwrap();
    assert_eq!(res, ModalKeyResult::SessionSelected { session_id });
    assert!(controller.state().active_modal().is_none());
    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn session_selector_modal_ctrl_d_deletes_session() {
    let temp_dir = std::env::temp_dir().join(format!("test_sessions_del_{}", uuid::Uuid::new_v4()));
    let manager = rho_harness_core::session::SessionManager::new(&temp_dir, None).unwrap();
    let session_id = manager.session_id.clone();

    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    open_session_selector(&temp_dir, &mut controller);

    let ctrl_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
    let res = handle_modal_key(&mut controller, ctrl_d, &mut None).unwrap();
    assert_eq!(res, ModalKeyResult::SessionDeleted { session_id });
    assert!(controller.state().active_modal().unwrap().options.is_empty());
    let _ = std::fs::remove_dir_all(temp_dir);
}

// =========================================================================
// Tree Modal Tests
// =========================================================================

fn make_tree_with_node(id: &str, label: Option<&str>) -> rho_harness_core::session::tree::SessionTree {
    let mut tree = rho_harness_core::session::tree::SessionTree::new();
    tree.add_node(rho_harness_core::session::tree::TreeNodeData {
        id: id.into(),
        parent_id: None,
        timestamp: chrono::Utc::now(),
        kind: rho_harness_core::session::tree::TreeNodeKind::UserTurn,
        messages: vec![rig::message::Message::user("Hello")],
        label: label.map(Into::into),
        metadata: None,
    });
    tree
}

#[test]
fn tree_selector_modal_selection() {
    let tree = make_tree_with_node("node-1", Some("checkpoint-1"));
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    open_tree_selector(&tree, &mut controller);

    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let res = handle_modal_key(&mut controller, enter, &mut None).unwrap();
    assert_eq!(
        res,
        ModalKeyResult::TreeNodeSelected {
            node_id: "node-1".into()
        }
    );
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn tree_selector_modal_shift_l_labels_checkpoint() {
    let tree = make_tree_with_node("node-42", None);
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    open_tree_selector(&tree, &mut controller);

    let shift_l = KeyEvent::new(KeyCode::Char('L'), KeyModifiers::SHIFT);
    assert_eq!(
        handle_modal_key(&mut controller, shift_l, &mut None).unwrap(),
        ModalKeyResult::Handled
    );

    for c in ['a', 'b'] {
        let key = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        let _ = handle_modal_key(&mut controller, key, &mut None).unwrap();
    }

    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let res = handle_modal_key(&mut controller, enter, &mut None).unwrap();
    assert_eq!(
        res,
        ModalKeyResult::NodeLabelUpdated {
            node_id: "node-42".into(),
            label: "ab".into()
        }
    );
}

// =========================================================================
// Claude Modal Tests
// =========================================================================

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
        model: "claude-sonnet-4-6".into(),
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

    let claude_opt = modal.options.iter().find(|o| o.label == "claude-sonnet-4-6").unwrap();
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
        focused: true,
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
                ("claude-sonnet-4-6", "claude", false)
            );
        }
        _ => panic!("expected ModelSelected"),
    }
}

#[test]
fn guard_model_selector_lifecycle() {
    let (_dir, session) = setup_claude_session();
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_guard_model_selector(&session, &mut controller);

    let modal = controller.state().active_modal().unwrap();
    assert_eq!(modal.title, "Select Guard Model");
    assert_eq!(modal.options[0].label, "None");

    let res = handle_modal_key(
        &mut controller,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &mut None,
    )
    .unwrap();
    assert_eq!(res, ModalKeyResult::Handled);
    assert!(controller.state().active_modal().is_none());

    super::super::modal::open_guard_model_selector(&session, &mut controller);
    let res = handle_modal_key(
        &mut controller,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mut None,
    )
    .unwrap();
    match res {
        ModalKeyResult::GuardModelSelected { model, provider } => {
            assert_eq!((model.as_str(), provider.as_str()), ("None", "none"));
        }
        _ => panic!("expected GuardModelSelected"),
    }

    super::super::modal::open_guard_model_selector(&session, &mut controller);
    let _ = handle_modal_key(
        &mut controller,
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        &mut None,
    )
    .unwrap();
    let res = handle_modal_key(
        &mut controller,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mut None,
    )
    .unwrap();
    match res {
        ModalKeyResult::GuardModelSelected { model, provider } => {
            assert!(!model.is_empty());
            assert!(!provider.is_empty());
        }
        _ => panic!("expected GuardModelSelected"),
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

#[test]
fn search_engine_selector_modal_lifecycle() {
    let temp = tempfile::tempdir().unwrap();
    let session = setup_model_selector_session(temp.path().to_path_buf());
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();

    super::super::modal::open_search_engine_selector(&session, &mut controller);
    assert_eq!(controller.state().active_modal().unwrap().title, "Select Search Engine");

    let key = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(
        super::super::modal::handle_modal_key(&mut controller, key, &mut None).unwrap(),
        super::super::modal::ModalKeyResult::Handled
    );
    assert_eq!(controller.state().active_modal().unwrap().selected, 1);

    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let res = super::super::modal::handle_modal_key(&mut controller, enter, &mut None).unwrap();
    assert_eq!(
        res,
        super::super::modal::ModalKeyResult::SearchEngineSelected {
            engine: "duckduckgo".to_string()
        }
    );
    assert!(controller.state().active_modal().is_none());
}
