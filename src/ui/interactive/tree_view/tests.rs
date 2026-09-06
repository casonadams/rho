use chrono::Utc;
use rho_harness_core::session::tree::{SessionTree, TreeNodeData, TreeNodeKind};
use rig::message::Message;

use super::{build_tree_display, render_tree_ascii};

fn sample_two_node_tree() -> SessionTree {
    let mut tree = SessionTree::new();
    tree.add_node(TreeNodeData {
        id: "root-1".to_string(),
        parent_id: None,
        timestamp: Utc::now(),
        kind: TreeNodeKind::UserTurn,
        messages: vec![Message::user("Root prompt")],
        label: Some("root".to_string()),
        metadata: None,
    });
    tree.add_node(TreeNodeData {
        id: "child-1".to_string(),
        parent_id: Some("root-1".to_string()),
        timestamp: Utc::now(),
        kind: TreeNodeKind::AssistantTurn,
        messages: vec![Message::assistant("Child answer")],
        label: None,
        metadata: None,
    });
    tree
}

#[test]
fn test_tree_display_hierarchy() {
    let tree = sample_two_node_tree();
    let display = build_tree_display(&tree);
    assert_eq!(display.len(), 2);
    assert_eq!((display[0].depth, display[0].label.as_deref()), (0, Some("root")));
    assert_eq!((display[1].depth, display[1].is_active), (1, true));
}

#[test]
fn test_tree_display_ascii() {
    let tree = sample_two_node_tree();
    let ascii = render_tree_ascii(&tree);
    assert!(ascii.contains("User: \"Root prompt\""));
    assert!(ascii.contains("Assistant: \"Child answer\"") && ascii.contains("[ACTIVE]"));
}
