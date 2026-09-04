use super::*;
use rig::message::{
    AssistantContent, Message, ToolCall, ToolCallId, ToolFunction, ToolResult, ToolResultContent, UserContent,
};

#[test]
fn test_estimate_text_tokens_exact_or_fallback() {
    let sample = "The quick brown fox jumps over the lazy dog.";
    let tokens = estimate_text_tokens(sample, "gpt-4");
    assert!(tokens > 0 && tokens < 20);

    let char_tokens = estimate_char_tokens(sample);
    assert!(char_tokens > 0);
}

#[test]
fn test_estimate_message_tokens() {
    let msg = Message::User {
        content: vec![
            UserContent::text("Hello world!"),
            UserContent::ToolResult(ToolResult {
                call: ToolCallId::new_or_mint("call-1"),
                provider: None,
                name: "read".to_string(),
                content: vec![ToolResultContent::Text(rig::message::Text::new(
                    "file content sample line",
                ))],
            }),
        ],
    };

    let tokens = estimate_message_tokens(&msg, "claude-3-7-sonnet");
    assert!(tokens >= 5);
}

#[test]
fn test_calculate_context_tokens_and_should_compact() {
    let messages = vec![
        Message::user("Initial prompt"),
        Message::assistant("Response 1"),
        Message::user("Trailing query"),
    ];

    let stats_no_anchor = calculate_context_tokens(&messages, None, "gpt-4");
    assert!(stats_no_anchor.total_tokens > 0);
    assert_eq!(stats_no_anchor.usage_anchor_tokens, 0);

    let stats_anchored = calculate_context_tokens(&messages, Some((1, 500)), "gpt-4");
    assert!(stats_anchored.total_tokens > 500);
    assert_eq!(stats_anchored.usage_anchor_tokens, 500);

    let window = 200_000;
    let reserve = 16_384;
    assert!(!should_compact(50_000, window, reserve));
    assert!(should_compact(190_000, window, reserve));
}

#[test]
fn test_find_token_cut_point_and_tool_pair_preservation() {
    let messages = vec![
        Message::user("User message 1"),
        Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(ToolCall::new(
                ToolCallId::new_or_mint("call-1"),
                ToolFunction::new("read".to_string(), serde_json::json!({"path": "test.txt"})),
            ))],
        },
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: ToolCallId::new_or_mint("call-1"),
                provider: None,
                name: "read".to_string(),
                content: vec![ToolResultContent::Text(rig::message::Text::new("test content"))],
            })],
        },
        Message::assistant("Assistant final"),
    ];

    let cut_idx = find_token_cut_point(&messages, 10, "gpt-4");
    assert!(cut_idx <= 1);
}

#[test]
fn test_context_window_size() {
    assert_eq!(context_window_size("claude-sonnet-4-6"), 200_000);
    assert_eq!(context_window_size("claude-opus-4-6"), 200_000);
    assert_eq!(context_window_size("claude-3-7-sonnet"), 200_000);
    assert_eq!(context_window_size("gemini-2.5-pro"), 2_000_000);
    assert_eq!(context_window_size("gemini-2.5-flash"), 1_000_000);
    assert_eq!(context_window_size("gpt-5.6"), 372_000);
    assert_eq!(context_window_size("gpt-5.4"), 272_000);
    assert_eq!(context_window_size("unknown-model"), 128_000);
}
