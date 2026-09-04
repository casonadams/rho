use super::super::files::{extract_file_ops, normalize_path, render_file_lists_xml};
use super::super::types::CompactionDetails;
use rig::message::{AssistantContent, Message, ToolCall, ToolCallId, ToolFunction};

#[test]
fn test_normalize_path() {
    assert_eq!(normalize_path("  ./src/main.rs  "), "src/main.rs");
    assert_eq!(normalize_path("src/lib.rs"), "src/lib.rs");
    assert_eq!(normalize_path("./Cargo.toml"), "Cargo.toml");
    assert_eq!(normalize_path(""), "");
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
