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
fn test_calculate_context_tokens() {
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
}

#[test]
fn test_should_compact_thresholds() {
    let window = 200_000;
    let cases = [
        (50_000, 16_384, false),
        (183_616, 16_384, false),
        (183_617, 16_384, true),
        (195_000, 16_384, true),
        (180_000, 20_000, false),
        (180_001, 20_000, true),
    ];
    for (tokens, reserve, expected) in cases {
        assert_eq!(should_compact(tokens, window, reserve), expected);
    }
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

    let cut = find_token_cut_point(&messages, 10, "gpt-4");
    assert!(cut.cut_index <= 1);
}

#[test]
fn test_context_window_size() {
    let cases = [
        ("claude-sonnet-4-6", 1_000_000),
        ("claude-opus-4-6", 1_000_000),
        ("claude-fable-5.1", 1_000_000),
        ("claude-haiku-4-5", 200_000),
        ("gemini-2.5-pro", 2_000_000),
        ("gemini-2.5-flash", 1_000_000),
        ("gpt-6-astra", 1_050_000),
        ("gpt-5.6", 372_000),
        ("gpt-5.4", 272_000),
        ("unknown-model", 128_000),
    ];
    for (model, expected) in cases {
        assert_eq!(context_window_size(model), expected);
    }
}

#[test]
fn context_window_size_is_provider_aware_for_gpt_6_astra() {
    assert_eq!(context_window_size_for_provider("gpt-6-astra", "openai"), 1_050_000);
    assert_eq!(context_window_size_for_provider("gpt-6-astra", "chatgpt"), 372_000);
}
