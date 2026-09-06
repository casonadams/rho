use super::common::HistoryTerminal;
use crate::repl::live::batch::LiveBatch;
use crate::repl::live::idle::shortcut::{IdleShortcutContext, handle_shortcut_action};
use crate::ui::interactive::{InputAction, InteractiveState, ModalMode, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

async fn setup_shortcut_harness() -> (
    TerminalController<HistoryTerminal>,
    crate::repl::ReplSession,
    crate::engine::AgentEngine,
) {
    let controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let config = rho_harness_core::config::Config::default();
    let auth = crate::auth::AuthStore::default();
    let session = crate::repl::ReplSession::new(config.clone(), auth.clone(), None);
    let engine = crate::platform::agent_engine(config, auth, None).await.unwrap();
    (controller, session, engine)
}

#[tokio::test]
async fn test_idle_shortcut_ctrl_c_clears_editor_without_opening_tree() {
    let (mut controller, mut session, mut engine) = setup_shortcut_harness().await;
    controller.state_mut().editor_mut().set_text("draft message to clear");
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

    let ctx2 = IdleShortcutContext {
        controller: &mut controller,
        session: &mut session,
        engine: &mut engine,
        last_escape_time: &mut last_escape_time,
    };
    handle_shortcut_action(InputAction::Clear, ctx2, &mut batch)
        .await
        .unwrap();
    assert!(controller.state().active_modal().is_none() && last_escape_time.is_none());
}

#[tokio::test]
async fn test_idle_shortcut_double_escape_opens_tree_when_empty() {
    let (mut controller, mut session, mut engine) = setup_shortcut_harness().await;
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
    assert!(last_escape_time.is_some() && controller.state().active_modal().is_none());

    let ctx2 = IdleShortcutContext {
        controller: &mut controller,
        session: &mut session,
        engine: &mut engine,
        last_escape_time: &mut last_escape_time,
    };
    handle_shortcut_action(InputAction::Cancel, ctx2, &mut batch)
        .await
        .unwrap();
    assert_eq!(controller.state().active_modal().unwrap().title, "Conversation Tree");
}

#[test]
fn test_modal_filter_ctrl_c_clears_query_and_esc_dismisses() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let session = crate::repl::ReplSession::new(
        rho_harness_core::config::Config::default(),
        crate::auth::AuthStore::default(),
        None,
    );
    super::super::modal::open_model_selector(&session, &mut controller);
    controller.state_mut().active_modal_mut().unwrap().set_filter("claude");

    let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(
        super::super::modal::handle_modal_key(&mut controller, ctrl_c, &mut None).unwrap(),
        super::super::modal::ModalKeyResult::Handled
    );
    assert_eq!(controller.state().active_modal().unwrap().filter_query, "");

    let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(
        super::super::modal::handle_modal_key(&mut controller, esc, &mut None).unwrap(),
        super::super::modal::ModalKeyResult::Handled
    );
    assert!(controller.state().active_modal().is_none());
}

fn single_node_tree(id: &str) -> rho_harness_core::session::tree::SessionTree {
    let mut tree = rho_harness_core::session::tree::SessionTree::new();
    tree.add_node(rho_harness_core::session::tree::TreeNodeData {
        id: id.into(),
        parent_id: None,
        timestamp: chrono::Utc::now(),
        kind: rho_harness_core::session::tree::TreeNodeKind::UserTurn,
        messages: vec![rig::message::Message::user("test")],
        label: None,
        metadata: None,
    });
    tree
}

fn send_modal_key(controller: &mut TerminalController<HistoryTerminal>, event: KeyEvent) {
    let _ = super::super::modal::handle_modal_key(controller, event, &mut None).unwrap();
}

#[test]
fn test_modal_tree_input_ctrl_c_clears_input_text() {
    let tree = single_node_tree("node-100");
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_tree_selector(&tree, &mut controller);

    send_modal_key(&mut controller, KeyEvent::new(KeyCode::Char('L'), KeyModifiers::SHIFT));
    send_modal_key(&mut controller, KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    assert_eq!(controller.state().active_modal().unwrap().input.text(), "x");

    send_modal_key(
        &mut controller,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    );
    let modal = controller.state().active_modal().unwrap();
    assert_eq!(modal.input.text(), "");
    assert!(matches!(modal.mode, ModalMode::Input { .. }));
}
