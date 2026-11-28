use super::super::serialize::{MAX_TOOL_RESULT_CHARS, serialize_conversation};
use crate::model::{AssistantContent, ChatMessage, ToolCall, ToolFunction, ToolResult, UserContent};
use serde_json::json;

#[test]
fn test_serialize_conversation_basic() {
    let messages = vec![
        ChatMessage::System {
            content: "You are an assistant.".to_string(),
        },
        ChatMessage::user("Please review this code."),
        ChatMessage::assistant("Looks good to me."),
    ];

    let transcript = serialize_conversation(&messages);
    assert!(transcript.contains("[System]: You are an assistant."));
    assert!(transcript.contains("[User]: Please review this code."));
    assert!(transcript.contains("[Assistant]: Looks good to me."));
}

fn tool_result_msg(id: &str, text: &str) -> ChatMessage {
    ChatMessage::User {
        content: vec![UserContent::ToolResult(ToolResult::new(id, "read", text))],
    }
}

#[test]
fn test_serialize_tool_short_and_exact() {
    let short_msg = tool_result_msg("call-1", "short output");
    let transcript = serialize_conversation(&[short_msg]);
    assert_eq!(transcript, "[Tool result]: short output");
    assert!(!transcript.contains("truncated"));

    let exact_2000: String = "a".repeat(MAX_TOOL_RESULT_CHARS);
    let exact_msg = tool_result_msg("call-2", &exact_2000);
    let transcript_exact = serialize_conversation(&[exact_msg]);
    assert_eq!(transcript_exact, format!("[Tool result]: {exact_2000}"));
    assert!(!transcript_exact.contains("truncated"));
}

#[test]
fn test_serialize_tool_oversized_and_unicode() {
    let oversized: String = "x".repeat(2500);
    let oversized_msg = tool_result_msg("call-3", &oversized);
    let transcript_over = serialize_conversation(&[oversized_msg]);
    let body = transcript_over.strip_prefix("[Tool result]: ").unwrap();
    let (kept, notice) = body.split_once('\n').unwrap();
    assert_eq!(kept.chars().count(), MAX_TOOL_RESULT_CHARS);
    assert_eq!(notice, "[... truncated 500 characters ...]");

    let unicode_msg = tool_result_msg("call-4", &"🦀".repeat(2100));
    let transcript_uni = serialize_conversation(&[unicode_msg]);
    assert!(transcript_uni.contains("[... truncated 100 characters ...]"));
}

#[test]
fn test_serialize_conversation_assistant_tool_call() {
    let msg = ChatMessage::Assistant {
        id: None,
        content: vec![
            AssistantContent::text("Let me inspect the file."),
            AssistantContent::ToolCall(ToolCall::new(
                "call-5",
                ToolFunction::new(
                    "read",
                    serde_json::json!({
                        "path": "src/main.rs"
                    }),
                ),
            )),
        ],
    };

    let transcript = serialize_conversation(&[msg]);
    assert!(transcript.contains("[Assistant]: Let me inspect the file."));
    assert!(transcript.contains("[Assistant tool call]: read({\"path\":\"src/main.rs\"})"));
}

fn named_tool_result_msg(id: &str, tool_name: &str, text: &str) -> ChatMessage {
    ChatMessage::User {
        content: vec![UserContent::ToolResult(ToolResult::new(id, tool_name, text))],
    }
}

#[test]
fn test_serialize_conversation_prunes_tool_results_to_last_three() {
    let mut messages = Vec::new();
    for i in 1..=10 {
        messages.push(named_tool_result_msg(
            &format!("call-{i}"),
            &format!("tool_{i}"),
            &format!("payload_alpha_{i}_omega"),
        ));
    }

    let transcript = serialize_conversation(&messages);

    // The first 7 tool results should be replaced with stubs
    for i in 1..=7 {
        assert!(
            transcript.contains(&format!("[tool result: tool_{i}]")),
            "Expected stub for tool_{i}"
        );
        assert!(
            !transcript.contains(&format!("payload_alpha_{i}_omega")),
            "Expected output of tool_{i} to be pruned"
        );
    }

    // The last 3 tool results should be kept in full
    for i in 8..=10 {
        assert!(
            transcript.contains(&format!("[Tool result]: payload_alpha_{i}_omega")),
            "Expected full output for tool_{i}"
        );
    }
}

#[test]
fn test_serialize_conversation_preserves_three_or_fewer_tool_results() {
    let messages = vec![
        named_tool_result_msg("call-1", "bash", "listing 1"),
        named_tool_result_msg("call-2", "read", "file contents"),
    ];

    let transcript = serialize_conversation(&messages);
    assert!(transcript.contains("[Tool result]: listing 1"));
    assert!(transcript.contains("[Tool result]: file contents"));
    assert!(!transcript.contains("[tool result:"));
}

#[test]
fn large_tool_arguments_are_replaced_by_a_line_count_summary() {
    let thousand_lines = (1..=1000)
        .map(|i| format!("line {i} of code"))
        .collect::<Vec<_>>()
        .join("\n");
    let large_old = "old line to replace\n".repeat(30);
    let large_new = "new replacement line\n".repeat(30);
    let large_script = "echo 'line of script'\n".repeat(40);
    let large_text = "content line\n".repeat(30);
    let cases = [
        (
            "write",
            json!({"path": "src/large.rs", "content": thousand_lines}),
            1000,
            "line 500 of code",
        ),
        (
            "edit",
            json!({"path": "src/app.rs", "edits": [{"oldText": &large_old, "newText": &large_new}]}),
            30,
            large_old.as_str(),
        ),
        ("bash", json!({"command": &large_script}), 40, large_script.as_str()),
        (
            "write_file",
            json!({"path": "test.txt", "content": &large_text}),
            30,
            large_text.as_str(),
        ),
    ];
    for (tool, args, lines, leaked) in cases {
        let msg = ChatMessage::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(ToolCall::new(
                "call-1",
                ToolFunction::new(tool, args.clone()),
            ))],
        };

        let transcript = serialize_conversation(&[msg]);
        assert!(
            transcript.contains(&format!("[Assistant tool call]: {tool}(")),
            "{tool}"
        );
        assert!(transcript.contains(&format!("[truncated: {lines} lines, ")), "{tool}");
        assert!(!transcript.contains(leaked), "{tool}");
        if let Some(path) = args.get("path") {
            assert!(transcript.contains(&format!("\"path\":{path}")), "{tool}");
        }
    }
}

#[test]
fn test_serialize_conversation_small_write_and_edit_preserved() {
    let msg = ChatMessage::Assistant {
        id: None,
        content: vec![
            AssistantContent::ToolCall(ToolCall::new(
                "call-w",
                ToolFunction::new(
                    "write",
                    serde_json::json!({
                        "path": "hello.txt",
                        "content": "short text",
                    }),
                ),
            )),
            AssistantContent::ToolCall(ToolCall::new(
                "call-e",
                ToolFunction::new(
                    "edit",
                    serde_json::json!({
                        "path": "hello.txt",
                        "edits": [{
                            "oldText": "short",
                            "newText": "tiny",
                        }]
                    }),
                ),
            )),
        ],
    };

    let transcript = serialize_conversation(&[msg]);
    assert!(transcript.contains("\"content\":\"short text\""));
    assert!(transcript.contains("\"oldText\":\"short\""));
    assert!(transcript.contains("\"newText\":\"tiny\""));
    assert!(!transcript.contains("truncated"));
}
