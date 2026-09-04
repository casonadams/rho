use crate::session::tree::{TreeNodeData, TreeNodeKind};
use crate::tokens::{find_node_token_cut_point, find_token_cut_point};
use rig::message::{
    AssistantContent, Message, ToolCall, ToolCallId, ToolFunction, ToolResult, ToolResultContent, UserContent,
};

#[test]
fn test_find_token_cut_point_preserves_atomic_tool_pairs() {
    let messages = vec![
        Message::user("Turn 1 user request"),
        Message::assistant("Turn 1 assistant response"),
        Message::user("Turn 2 user request"),
        Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(ToolCall::new(
                ToolCallId::new_or_mint("c1"),
                ToolFunction::new("read".to_string(), serde_json::json!({"path": "src/main.rs"})),
            ))],
        },
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: ToolCallId::new_or_mint("c1"),
                provider: None,
                name: "read".to_string(),
                content: vec![ToolResultContent::Text(rig::message::Text::new("fn main() {}"))],
            })],
        },
        Message::assistant("Turn 2 final answer"),
    ];

    let cut = find_token_cut_point(&messages, 10, "claude-3-7-sonnet");
    assert!(cut.cut_index <= 3);
    assert_ne!(cut.cut_index, 4);
}

#[test]
fn test_find_token_cut_point_split_turn_detection() {
    let messages = vec![
        Message::user("User turn 1"),
        Message::assistant("Assistant turn 1"),
        Message::user("User turn 2"),
        Message::assistant("Assistant turn 2"),
    ];

    let cut_clean = find_token_cut_point(&messages, 20, "gpt-4");
    if cut_clean.cut_index == 2 {
        assert!(!cut_clean.is_split_turn);
    }

    let oversized_turn = vec![
        Message::user("User initial prompt"),
        Message::assistant("Assistant step 1: beginning analysis of the problem in great detail with many tokens."),
        Message::assistant("Assistant step 2: continuing the analysis and generating a very large response."),
    ];

    let cut_split = find_token_cut_point(&oversized_turn, 15, "gpt-4");
    assert!(cut_split.cut_index > 0);
    assert!(cut_split.is_split_turn);
}

#[test]
fn test_find_node_token_cut_point() {
    let now = chrono::Utc::now();
    let node1 = TreeNodeData {
        id: "node-1".to_string(),
        parent_id: None,
        timestamp: now,
        kind: TreeNodeKind::UserTurn,
        messages: vec![Message::user("Turn 1 user"), Message::assistant("Turn 1 assistant")],
        label: None,
        metadata: None,
    };
    let node2 = TreeNodeData {
        id: "node-2".to_string(),
        parent_id: Some("node-1".to_string()),
        timestamp: now,
        kind: TreeNodeKind::UserTurn,
        messages: vec![Message::user("Turn 2 user"), Message::assistant("Turn 2 assistant")],
        label: None,
        metadata: None,
    };

    let nodes = vec![&node1, &node2];
    let cut = find_node_token_cut_point(&nodes, 10, "gpt-4");
    assert!(cut.cut_index <= 3);
    assert!(cut.first_kept_node_id.is_some());
    let kept_id = cut.first_kept_node_id.unwrap();
    assert!(kept_id == "node-1" || kept_id == "node-2");
}
