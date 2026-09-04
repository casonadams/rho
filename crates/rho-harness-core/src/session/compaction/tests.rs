use rig::message::{
    AssistantContent, Message, ToolCall, ToolCallId, ToolFunction, ToolResult, ToolResultContent, UserContent,
};

use super::files::{extract_file_ops, normalize_path, render_file_lists_xml};
use super::serialize::{MAX_TOOL_RESULT_CHARS, serialize_conversation};
use super::types::CompactionDetails;
use crate::session::tree::{TreeNodeData, TreeNodeKind};
use crate::tokens::{find_node_token_cut_point, find_token_cut_point};

#[test]
fn test_normalize_path() {
    assert_eq!(normalize_path("  ./src/main.rs  "), "src/main.rs");
    assert_eq!(normalize_path("src/lib.rs"), "src/lib.rs");
    assert_eq!(normalize_path("./Cargo.toml"), "Cargo.toml");
    assert_eq!(normalize_path(""), "");
}

#[test]
fn test_serialize_conversation_basic() {
    let messages = vec![
        Message::System {
            content: "You are an assistant.".to_string(),
        },
        Message::user("Please review this code."),
        Message::assistant("Looks good to me."),
    ];

    let transcript = serialize_conversation(&messages);
    assert!(transcript.contains("[System]: You are an assistant."));
    assert!(transcript.contains("[User]: Please review this code."));
    assert!(transcript.contains("[Assistant]: Looks good to me."));
}

#[test]
fn test_serialize_conversation_tool_truncation() {
    let short_text = "short output";
    let short_msg = Message::User {
        content: vec![UserContent::ToolResult(ToolResult {
            call: ToolCallId::new_or_mint("call-1"),
            provider: None,
            name: "read".to_string(),
            content: vec![ToolResultContent::Text(rig::message::Text::new(short_text))],
        })],
    };

    let transcript = serialize_conversation(&[short_msg]);
    assert_eq!(transcript, format!("[Tool result]: {short_text}"));
    assert!(!transcript.contains("truncated"));

    let exact_2000: String = "a".repeat(MAX_TOOL_RESULT_CHARS);
    let exact_msg = Message::User {
        content: vec![UserContent::ToolResult(ToolResult {
            call: ToolCallId::new_or_mint("call-2"),
            provider: None,
            name: "read".to_string(),
            content: vec![ToolResultContent::Text(rig::message::Text::new(&exact_2000))],
        })],
    };

    let transcript_exact = serialize_conversation(&[exact_msg]);
    assert_eq!(transcript_exact, format!("[Tool result]: {exact_2000}"));
    assert!(!transcript_exact.contains("truncated"));

    let oversized: String = "x".repeat(2500);
    let oversized_msg = Message::User {
        content: vec![UserContent::ToolResult(ToolResult {
            call: ToolCallId::new_or_mint("call-3"),
            provider: None,
            name: "read".to_string(),
            content: vec![ToolResultContent::Text(rig::message::Text::new(&oversized))],
        })],
    };

    let transcript_over = serialize_conversation(&[oversized_msg]);
    assert!(transcript_over.starts_with("[Tool result]: "));
    let body = transcript_over.strip_prefix("[Tool result]: ").unwrap();
    let (kept, notice) = body.split_once('\n').unwrap();
    assert_eq!(kept.chars().count(), MAX_TOOL_RESULT_CHARS);
    assert_eq!(notice, "[... truncated 500 characters ...]");

    let unicode_text: String = "🦀".repeat(2100);
    let unicode_msg = Message::User {
        content: vec![UserContent::ToolResult(ToolResult {
            call: ToolCallId::new_or_mint("call-4"),
            provider: None,
            name: "read".to_string(),
            content: vec![ToolResultContent::Text(rig::message::Text::new(&unicode_text))],
        })],
    };
    let transcript_uni = serialize_conversation(&[unicode_msg]);
    assert!(transcript_uni.contains("[... truncated 100 characters ...]"));
}

#[test]
fn test_serialize_conversation_assistant_tool_call() {
    let msg = Message::Assistant {
        id: None,
        content: vec![
            AssistantContent::text("Let me inspect the file."),
            AssistantContent::ToolCall(ToolCall::new(
                ToolCallId::new_or_mint("call-1"),
                ToolFunction::new("read".to_string(), serde_json::json!({"path": "src/main.rs"})),
            )),
        ],
    };

    let transcript = serialize_conversation(&[msg]);
    assert!(transcript.contains("[Assistant]: Let me inspect the file."));
    assert!(transcript.contains("[Assistant tool call]: read({\"path\":\"src/main.rs\"})"));
}

#[test]
fn test_extract_file_ops_empty() {
    let details = extract_file_ops(&[], None);
    assert!(details.read_files.is_empty());
    assert!(details.modified_files.is_empty());
}

#[test]
fn test_extract_file_ops_single_turn() {
    let messages = vec![Message::Assistant {
        id: None,
        content: vec![
            AssistantContent::ToolCall(ToolCall::new(
                ToolCallId::new_or_mint("c1"),
                ToolFunction::new("read".to_string(), serde_json::json!({"path": " ./src/read.rs "})),
            )),
            AssistantContent::ToolCall(ToolCall::new(
                ToolCallId::new_or_mint("c2"),
                ToolFunction::new("write".to_string(), serde_json::json!({"path": "src/written.rs"})),
            )),
            AssistantContent::ToolCall(ToolCall::new(
                ToolCallId::new_or_mint("c3"),
                ToolFunction::new("edit".to_string(), serde_json::json!({"path": "./src/edited.rs"})),
            )),
        ],
    }];

    let details = extract_file_ops(&messages, None);
    assert_eq!(details.read_files, vec!["src/read.rs"]);
    assert_eq!(details.modified_files, vec!["src/edited.rs", "src/written.rs"]);
}

#[test]
fn test_extract_file_ops_modified_supersedes_read() {
    let messages = vec![Message::Assistant {
        id: None,
        content: vec![
            AssistantContent::ToolCall(ToolCall::new(
                ToolCallId::new_or_mint("c1"),
                ToolFunction::new("read".to_string(), serde_json::json!({"path": "src/shared.rs"})),
            )),
            AssistantContent::ToolCall(ToolCall::new(
                ToolCallId::new_or_mint("c2"),
                ToolFunction::new("edit".to_string(), serde_json::json!({"path": "src/shared.rs"})),
            )),
        ],
    }];

    let details = extract_file_ops(&messages, None);
    assert!(details.read_files.is_empty());
    assert_eq!(details.modified_files, vec!["src/shared.rs"]);
}

#[test]
fn test_extract_file_ops_accumulate_with_prior() {
    let prior = CompactionDetails {
        read_files: vec!["README.md".to_string(), "docs/spec.md".to_string()],
        modified_files: vec!["src/lib.rs".to_string()],
    };

    let messages = vec![Message::Assistant {
        id: None,
        content: vec![
            AssistantContent::ToolCall(ToolCall::new(
                ToolCallId::new_or_mint("c1"),
                ToolFunction::new("edit".to_string(), serde_json::json!({"path": "docs/spec.md"})),
            )),
            AssistantContent::ToolCall(ToolCall::new(
                ToolCallId::new_or_mint("c2"),
                ToolFunction::new("read".to_string(), serde_json::json!({"path": "Cargo.toml"})),
            )),
        ],
    }];

    let details = extract_file_ops(&messages, Some(&prior));
    assert_eq!(details.read_files, vec!["Cargo.toml", "README.md"]);
    assert_eq!(details.modified_files, vec!["docs/spec.md", "src/lib.rs"]);
}

#[test]
fn test_render_file_lists_xml() {
    let details = CompactionDetails {
        read_files: vec!["src/bar.rs".to_string(), "src/foo.rs".to_string()],
        modified_files: vec!["src/baz.rs".to_string()],
    };

    let xml = render_file_lists_xml(&details);
    let expected =
        "<read-files>\nsrc/bar.rs\nsrc/foo.rs\n</read-files>\n\n<modified-files>\nsrc/baz.rs\n</modified-files>";
    assert_eq!(xml, expected);

    let read_only = CompactionDetails {
        read_files: vec!["src/bar.rs".to_string()],
        modified_files: vec![],
    };
    assert_eq!(
        render_file_lists_xml(&read_only),
        "<read-files>\nsrc/bar.rs\n</read-files>"
    );

    let mod_only = CompactionDetails {
        read_files: vec![],
        modified_files: vec!["src/baz.rs".to_string()],
    };
    assert_eq!(
        render_file_lists_xml(&mod_only),
        "<modified-files>\nsrc/baz.rs\n</modified-files>"
    );

    let empty = CompactionDetails::default();
    assert_eq!(render_file_lists_xml(&empty), "");
}

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
