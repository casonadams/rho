use chrono::Utc;
use rho_harness_core::session::tree::{SessionTree, TreeNodeData, TreeNodeKind};
use rig::message::Message;

use super::{build_tree_display, render_tree_ascii};

#[test]
fn test_tree_display_hierarchy_and_active_marker() {
    let mut tree = SessionTree::new();
    let root = TreeNodeData {
        id: "root-1".to_string(),
        parent_id: None,
        timestamp: Utc::now(),
        kind: TreeNodeKind::UserTurn,
        messages: vec![Message::user("Root prompt")],
        label: Some("root".to_string()),
        metadata: None,
    };
    tree.add_node(root);

    let child = TreeNodeData {
        id: "child-1".to_string(),
        parent_id: Some("root-1".to_string()),
        timestamp: Utc::now(),
        kind: TreeNodeKind::AssistantTurn,
        messages: vec![Message::assistant("Child answer")],
        label: None,
        metadata: None,
    };
    tree.add_node(child);

    let display = build_tree_display(&tree);
    assert_eq!(display.len(), 2);
    assert_eq!(display[0].depth, 0);
    assert_eq!(display[0].label, Some("root".to_string()));
    assert_eq!(display[1].depth, 1);
    assert!(display[1].is_active);

    let ascii = render_tree_ascii(&tree);
    assert!(ascii.contains("User: \"Root prompt\""));
    assert!(ascii.contains("Assistant: \"Child answer\""));
    assert!(ascii.contains("[ACTIVE]"));
}
