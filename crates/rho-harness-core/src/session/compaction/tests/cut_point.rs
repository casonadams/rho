use crate::model::{AssistantContent, ChatMessage, ToolCall, ToolFunction, ToolResult, UserContent};
use crate::session::tree::{TreeNodeData, TreeNodeKind};
use crate::tokens::{find_node_token_cut_point, find_token_cut_point};

#[test]
fn test_find_token_cut_point_preserves_atomic_tool_pairs() {
    let messages = vec![
        ChatMessage::user("Turn 1 user request"),
        ChatMessage::assistant("Turn 1 assistant response"),
        ChatMessage::user("Turn 2 user request"),
        ChatMessage::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(ToolCall::new(
                "c1",
                ToolFunction::new("read", serde_json::json!({"path": "src/main.rs"})),
            ))],
        },
        ChatMessage::User {
            content: vec![UserContent::ToolResult(ToolResult::new("c1", "read", "fn main() {}"))],
        },
        ChatMessage::assistant("Turn 2 final answer"),
    ];

    let cut = find_token_cut_point(&messages, 20, "claude-3-7-sonnet");
    assert!(cut.cut_index <= 3);
    assert_ne!(cut.cut_index, 4);
}

#[test]
fn test_find_token_cut_point_split_turn_detection() {
    let messages = vec![
        ChatMessage::user("User turn 1"),
        ChatMessage::assistant("Assistant turn 1"),
        ChatMessage::user("User turn 2"),
        ChatMessage::assistant("Assistant turn 2"),
    ];

    let cut_clean = find_token_cut_point(&messages, 20, "gpt-4");
    if cut_clean.cut_index == 2 {
        assert!(!cut_clean.is_split_turn);
    }

    let oversized_turn = vec![
        ChatMessage::user("User initial prompt"),
        ChatMessage::assistant("Assistant step 1: beginning analysis of the problem in great detail with many tokens."),
        ChatMessage::assistant("Assistant step 2: continuing the analysis and generating a very large response."),
    ];

    let cut_split = find_token_cut_point(&oversized_turn, 15, "gpt-4");
    assert!(cut_split.cut_index > 0);
    assert!(cut_split.is_split_turn);
}

fn sample_cut_nodes() -> (TreeNodeData, TreeNodeData) {
    let now = chrono::Utc::now();
    let node1 = TreeNodeData {
        id: "node-1".to_string(),
        parent_id: None,
        timestamp: now,
        kind: TreeNodeKind::UserTurn,
        messages: vec![
            ChatMessage::user("Turn 1 user"),
            ChatMessage::assistant("Turn 1 assistant"),
        ],
        label: None,
        metadata: None,
    };
    let node2 = TreeNodeData {
        id: "node-2".to_string(),
        parent_id: Some("node-1".to_string()),
        timestamp: now,
        kind: TreeNodeKind::UserTurn,
        messages: vec![
            ChatMessage::user("Turn 2 user"),
            ChatMessage::assistant("Turn 2 assistant"),
        ],
        label: None,
        metadata: None,
    };
    (node1, node2)
}

#[test]
fn test_find_node_token_cut_point() {
    let (node1, node2) = sample_cut_nodes();
    let nodes = vec![&node1, &node2];
    let cut = find_node_token_cut_point(&nodes, 10, "gpt-4");
    assert!(cut.cut_index <= 3);
    assert!(cut.first_kept_node_id.is_some());
    assert!(cut.first_kept_message_index.is_some());
    let kept_id = cut.first_kept_node_id.unwrap();
    assert!(kept_id == "node-1" || kept_id == "node-2");
}

#[test]
fn test_message_position_at() {
    let (node1, node2) = sample_cut_nodes();
    let nodes = vec![&node1, &node2];
    let expected = [
        (0, Some(("node-1".to_string(), 0))),
        (1, Some(("node-1".to_string(), 1))),
        (2, Some(("node-2".to_string(), 0))),
        (3, Some(("node-2".to_string(), 1))),
        (4, None),
    ];
    for (idx, exp) in expected {
        assert_eq!(crate::tokens::message_position_at(&nodes, idx), exp);
    }
}
