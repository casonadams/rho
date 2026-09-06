use super::super::serialize::{MAX_TOOL_RESULT_CHARS, serialize_conversation};
use rig::message::{
    AssistantContent, Message, ToolCall, ToolCallId, ToolFunction, ToolResult, ToolResultContent, UserContent,
};

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

fn tool_result_msg(id: &str, text: &str) -> Message {
    Message::User {
        content: vec![UserContent::ToolResult(ToolResult {
            call: ToolCallId::new_or_mint(id),
            provider: None,
            name: "read".to_string(),
            content: vec![ToolResultContent::Text(rig::message::Text::new(text))],
        })],
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
    let msg = Message::Assistant {
        id: None,
        content: vec![
            AssistantContent::text("Let me inspect the file."),
            AssistantContent::ToolCall(ToolCall::new(
                ToolCallId::new_or_mint("call-5"),
                ToolFunction::new(
                    "read".to_string(),
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
