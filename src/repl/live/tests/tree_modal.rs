use super::common::HistoryTerminal;
use crate::ui::interactive::{InteractiveState, TerminalController};

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
    super::super::modal::open_tree_selector(&tree, &mut controller);

    let enter = crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
    let res = super::super::modal::handle_modal_key(&mut controller, enter, &mut None).unwrap();
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
    let tree = make_tree_with_node("node-42", None);
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    super::super::modal::open_tree_selector(&tree, &mut controller);

    let shift_l = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('L'),
        crossterm::event::KeyModifiers::SHIFT,
    );
    assert_eq!(
        super::super::modal::handle_modal_key(&mut controller, shift_l, &mut None).unwrap(),
        super::super::modal::ModalKeyResult::Handled
    );

    for c in ['a', 'b'] {
        let key =
            crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Char(c), crossterm::event::KeyModifiers::NONE);
        let _ = super::super::modal::handle_modal_key(&mut controller, key, &mut None).unwrap();
    }

    let enter = crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
    let res = super::super::modal::handle_modal_key(&mut controller, enter, &mut None).unwrap();
    assert_eq!(
        res,
        super::super::modal::ModalKeyResult::NodeLabelUpdated {
            node_id: "node-42".into(),
            label: "ab".into()
        }
    );
}
