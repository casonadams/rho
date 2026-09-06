use super::super::fallback::generate_fallback_summary;
use rig::message::{
    AssistantContent, Message, ToolCall, ToolCallId, ToolFunction, ToolResult, ToolResultContent, UserContent,
};

#[test]
fn test_fallback_summary_empty_messages() {
    let summary = generate_fallback_summary(&[], None, None);
    let expected = [
        "## Goal\n(none)",
        "## Constraints & Preferences\n- (none)",
        "## Progress\n### Done\n- [x] (none)",
        "### In Progress\n- (none)",
        "### Blocked\n- (none)",
        "## Key Decisions\n- (none)",
        "## Next Steps\n1. Continue session work",
        "## Critical Context\n- (none)",
    ];
    for section in expected {
        assert!(summary.contains(section));
    }
}

#[test]
fn test_fallback_summary_extracts_goal_and_tool_calls() {
    let messages = vec![
        Message::user("Refactor database pooling"),
        Message::Assistant {
            id: None,
            content: vec![
                AssistantContent::text("Starting pool implementation."),
                AssistantContent::ToolCall(ToolCall::new(
                    ToolCallId::new_or_mint("c1"),
                    ToolFunction::new("edit".to_string(), serde_json::json!({"path": "./src/pool.rs"})),
                )),
                AssistantContent::ToolCall(ToolCall::new(
                    ToolCallId::new_or_mint("c2"),
                    ToolFunction::new("bash".to_string(), serde_json::json!({"command": "cargo check"})),
                )),
            ],
        },
    ];

    let summary = generate_fallback_summary(&messages, None, None);

    assert!(summary.contains("## Goal\nRefactor database pooling"));
    assert!(summary.contains("- [x] Modified `src/pool.rs`"));
    assert!(summary.contains("- [x] Ran command `cargo check`"));
    assert!(summary.contains("### Blocked\n- (none)"));
}

#[test]
fn test_fallback_summary_captures_errors_in_blocked() {
    let messages = vec![
        Message::user("Run migration script"),
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: ToolCallId::new_or_mint("call-err"),
                provider: None,
                name: "bash".to_string(),
                content: vec![ToolResultContent::Text(rig::message::Text::new(
                    "error: failed to connect to database at localhost:5432",
                ))],
            })],
        },
    ];

    let summary = generate_fallback_summary(&messages, None, None);

    assert!(
        summary.contains("### Blocked\n- Tool `bash` error: error: failed to connect to database at localhost:5432")
    );
}

fn sample_prior_summary() -> &'static str {
    "## Goal\nInitial authentication flow\n\n## Constraints & Preferences\n- Must use argon2 password hashing\n\n## Progress\n### Done\n- [x] Implemented password hasher\n\n### In Progress\n- [ ] Implement JWT tokens\n\n### Blocked\n- (none)\n\n## Key Decisions\n- **Argon2**: Chosen over bcrypt for memory hardness\n\n## Next Steps\n1. Finish JWT signing\n2. Add token middleware\n\n## Critical Context\n- Secret key configured via env\n"
}

#[test]
fn test_fallback_summary_preserves_prior_summary() {
    let messages = vec![Message::Assistant {
        id: None,
        content: vec![AssistantContent::ToolCall(ToolCall::new(
            ToolCallId::new_or_mint("c3"),
            ToolFunction::new("write".to_string(), serde_json::json!({"path": "src/jwt.rs"})),
        ))],
    }];
    let summary = generate_fallback_summary(&messages, Some(sample_prior_summary()), None);
    let expected = [
        "## Goal\nInitial authentication flow",
        "- Must use argon2 password hashing",
        "- [x] Implemented password hasher",
        "- [x] Modified `src/jwt.rs`",
        "- [ ] Implement JWT tokens",
        "- **Argon2**: Chosen over bcrypt for memory hardness",
        "1. Finish JWT signing",
        "2. Add token middleware",
        "- Secret key configured via env",
    ];
    for fragment in expected {
        assert!(summary.contains(fragment));
    }
}

#[test]
fn test_fallback_summary_with_custom_instructions() {
    let messages = vec![Message::user("Cleanup codebase")];
    let summary = generate_fallback_summary(&messages, None, Some("Strictly maintain 100% test coverage"));

    assert!(summary.contains("## Constraints & Preferences\n- Additional focus: Strictly maintain 100% test coverage"));
}

#[test]
fn test_fallback_summary_bounds_done_items() {
    let mut messages = Vec::new();
    for i in 0..25 {
        messages.push(Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(ToolCall::new(
                ToolCallId::new_or_mint(format!("call-{i}")),
                ToolFunction::new(
                    "write".to_string(),
                    serde_json::json!({"path": format!("src/file_{i}.rs")}),
                ),
            ))],
        });
    }

    let summary = generate_fallback_summary(&messages, None, None);
    assert!(!summary.contains("src/file_0.rs"));
    assert!(summary.contains("src/file_24.rs"));
}
