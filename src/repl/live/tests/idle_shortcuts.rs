use super::common::HistoryTerminal;
use crate::repl::live::batch::LiveBatch;
use crate::repl::live::idle::shortcut::{IdleShortcutContext, handle_shortcut_action};
use crate::ui::interactive::{InputAction, InteractiveState, ModalMode, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[tokio::test]
async fn test_idle_shortcut_ctrl_c_clears_editor_without_opening_tree() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    controller.state_mut().editor_mut().set_text("draft message to clear");

    let config = rho_harness_core::config::Config::default();
    let auth_store = crate::auth::AuthStore::default();
    let mut session = crate::repl::ReplSession::new(config.clone(), auth_store.clone(), None);
    let mut engine = crate::platform::agent_engine(config, auth_store, None).await.unwrap();
    let mut last_escape_time = None;
    let mut batch = LiveBatch::new();

    let ctx = IdleShortcutContext {
        controller: &mut controller,
        session: &mut session,
        engine: &mut engine,
        last_escape_time: &mut last_escape_time,
    };
    handle_shortcut_action(InputAction::Clear, ctx, &mut batch)
        .await
        .unwrap();

    assert_eq!(controller.state().editor().text(), "");
    assert!(last_escape_time.is_none());

    let ctx2 = IdleShortcutContext {
        controller: &mut controller,
        session: &mut session,
        engine: &mut engine,
        last_escape_time: &mut last_escape_time,
    };
    handle_shortcut_action(InputAction::Clear, ctx2, &mut batch)
        .await
        .unwrap();

    assert!(controller.state().active_modal().is_none());
    assert!(last_escape_time.is_none());
}

#[tokio::test]
async fn test_idle_shortcut_double_escape_opens_tree_when_empty() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let config = rho_harness_core::config::Config::default();
    let auth_store = crate::auth::AuthStore::default();
    let mut session = crate::repl::ReplSession::new(config.clone(), auth_store.clone(), None);
    let mut engine = crate::platform::agent_engine(config, auth_store, None).await.unwrap();
    let mut last_escape_time = None;
    let mut batch = LiveBatch::new();

    let ctx1 = IdleShortcutContext {
        controller: &mut controller,
        session: &mut session,
        engine: &mut engine,
        last_escape_time: &mut last_escape_time,
    };
    handle_shortcut_action(InputAction::Cancel, ctx1, &mut batch)
        .await
        .unwrap();

    assert!(last_escape_time.is_some());
    assert!(controller.state().active_modal().is_none());

    let ctx2 = IdleShortcutContext {
        controller: &mut controller,
        session: &mut session,
        engine: &mut engine,
        last_escape_time: &mut last_escape_time,
    };
    handle_shortcut_action(InputAction::Cancel, ctx2, &mut batch)
        .await
        .unwrap();

    let modal = controller.state().active_modal();
    assert!(modal.is_some());
    assert_eq!(modal.unwrap().title, "Conversation Tree");
}

#[test]
fn test_modal_filter_ctrl_c_clears_query_and_esc_dismisses() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let config = rho_harness_core::config::Config::default();
    let auth_store = crate::auth::AuthStore::default();
    let session = crate::repl::ReplSession::new(config, auth_store, None);

    super::super::modal::open_model_selector(&session, &mut controller);
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        modal.set_filter("claude");
    }
    assert_eq!(controller.state().active_modal().unwrap().filter_query, "claude");

    let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    let res = super::super::modal::handle_modal_key(&mut controller, ctrl_c, &mut None).unwrap();
    assert_eq!(res, super::super::modal::ModalKeyResult::Handled);
    assert!(controller.state().active_modal().is_some());
    assert_eq!(controller.state().active_modal().unwrap().filter_query, "");

    let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    let res = super::super::modal::handle_modal_key(&mut controller, esc, &mut None).unwrap();
    assert_eq!(res, super::super::modal::ModalKeyResult::Handled);
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn test_modal_tree_input_ctrl_c_clears_input_text() {
    let mut tree = rho_harness_core::session::tree::SessionTree::new();
    tree.add_node(rho_harness_core::session::tree::TreeNodeData {
        id: "node-100".into(),
        parent_id: None,
        timestamp: chrono::Utc::now(),
        kind: rho_harness_core::session::tree::TreeNodeKind::UserTurn,
        messages: vec![rig::message::Message::user("test")],
        label: None,
        metadata: None,
    });
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_tree_selector(&tree, &mut controller);

    let shift_l = KeyEvent::new(KeyCode::Char('L'), KeyModifiers::SHIFT);
    let _ = super::super::modal::handle_modal_key(&mut controller, shift_l, &mut None).unwrap();
    assert!(matches!(
        controller.state().active_modal().unwrap().mode,
        ModalMode::Input { .. }
    ));

    let char_x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, char_x, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().input.text(), "x");

    let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    let _ = super::super::modal::handle_modal_key(&mut controller, ctrl_c, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().input.text(), "");
    assert!(matches!(
        controller.state().active_modal().unwrap().mode,
        ModalMode::Input { .. }
    ));
}
