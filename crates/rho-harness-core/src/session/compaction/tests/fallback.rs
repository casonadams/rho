use super::super::fallback::generate_fallback_summary;
use rig::message::{
    AssistantContent, Message, ToolCall, ToolCallId, ToolFunction, ToolResult, ToolResultContent, UserContent,
};

#[test]
fn test_fallback_summary_empty_messages() {
    let summary = generate_fallback_summary(&[], None, None);

    assert!(summary.contains("## Goal\n(none)"));
    assert!(summary.contains("## Constraints & Preferences\n- (none)"));
    assert!(summary.contains("## Progress\n### Done\n- [x] (none)"));
    assert!(summary.contains("### In Progress\n- (none)"));
    assert!(summary.contains("### Blocked\n- (none)"));
    assert!(summary.contains("## Key Decisions\n- (none)"));
    assert!(summary.contains("## Next Steps\n1. Continue session work"));
    assert!(summary.contains("## Critical Context\n- (none)"));
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

#[test]
fn test_fallback_summary_preserves_prior_summary() {
    let prior = "\
## Goal
Initial authentication flow

## Constraints & Preferences
- Must use argon2 password hashing

## Progress
### Done
- [x] Implemented password hasher

### In Progress
- [ ] Implement JWT tokens

### Blocked
- (none)

## Key Decisions
- **Argon2**: Chosen over bcrypt for memory hardness

## Next Steps
1. Finish JWT signing
2. Add token middleware

## Critical Context
- Secret key configured via env
";

    let messages = vec![Message::Assistant {
        id: None,
        content: vec![AssistantContent::ToolCall(ToolCall::new(
            ToolCallId::new_or_mint("c3"),
            ToolFunction::new("write".to_string(), serde_json::json!({"path": "src/jwt.rs"})),
        ))],
    }];

    let summary = generate_fallback_summary(&messages, Some(prior), None);

    assert!(summary.contains("## Goal\nInitial authentication flow"));
    assert!(summary.contains("- Must use argon2 password hashing"));
    assert!(summary.contains("- [x] Implemented password hasher"));
    assert!(summary.contains("- [x] Modified `src/jwt.rs`"));
    assert!(summary.contains("- [ ] Implement JWT tokens"));
    assert!(summary.contains("- **Argon2**: Chosen over bcrypt for memory hardness"));
    assert!(summary.contains("1. Finish JWT signing"));
    assert!(summary.contains("2. Add token middleware"));
    assert!(summary.contains("- Secret key configured via env"));
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
