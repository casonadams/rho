use super::common::HistoryTerminal;
use crate::ui::interactive::{InteractiveState, TerminalController};

#[test]
fn tree_selector_modal_selection() {
    let mut tree = rho_harness_core::session::tree::SessionTree::new();
    tree.add_node(rho_harness_core::session::tree::TreeNodeData {
        id: "node-1".into(),
        parent_id: None,
        timestamp: chrono::Utc::now(),
        kind: rho_harness_core::session::tree::TreeNodeKind::UserTurn,
        messages: vec![rig::message::Message::user("Hello")],
        label: Some("checkpoint-1".into()),
        metadata: None,
    });
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();

    super::super::modal::open_tree_selector(&tree, &mut controller);
    assert_eq!(controller.state().active_modal().unwrap().title, "Conversation Tree");

    let enter_key =
        crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
    let res = super::super::modal::handle_modal_key(&mut controller, enter_key, &mut None).unwrap();
    assert_eq!(
        res,
        super::super::modal::ModalKeyResult::TreeNodeSelected {
            node_id: "node-1".into()
        }
    );
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn tree_selector_modal_shift_l_labels_checkpoint() {
    let mut tree = rho_harness_core::session::tree::SessionTree::new();
    tree.add_node(rho_harness_core::session::tree::TreeNodeData {
        id: "node-42".into(),
        parent_id: None,
        timestamp: chrono::Utc::now(),
        kind: rho_harness_core::session::tree::TreeNodeKind::UserTurn,
        messages: vec![rig::message::Message::user("Hello")],
        label: None,
        metadata: None,
    });
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_tree_selector(&tree, &mut controller);

    let shift_l = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('L'),
        crossterm::event::KeyModifiers::SHIFT,
    );
    let res = super::super::modal::handle_modal_key(&mut controller, shift_l, &mut None).unwrap();
    assert_eq!(res, super::super::modal::ModalKeyResult::Handled);
    assert!(matches!(
        controller.state().active_modal().unwrap().mode,
        crate::ui::interactive::ModalMode::Input { .. }
    ));

    let char_a = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('a'),
        crossterm::event::KeyModifiers::NONE,
    );
    let _ = super::super::modal::handle_modal_key(&mut controller, char_a, &mut None).unwrap();
    let char_b = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('b'),
        crossterm::event::KeyModifiers::NONE,
    );
    let _ = super::super::modal::handle_modal_key(&mut controller, char_b, &mut None).unwrap();

    let enter_key =
        crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
    let res = super::super::modal::handle_modal_key(&mut controller, enter_key, &mut None).unwrap();
    assert_eq!(
        res,
        super::super::modal::ModalKeyResult::NodeLabelUpdated {
            node_id: "node-42".into(),
            label: "ab".into(),
        }
    );
    assert!(controller.state().active_modal().is_none());
}
