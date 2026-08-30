use rho::tokens::{
    calculate_context_tokens, context_window_size, estimate_message_tokens, estimate_text_tokens, find_token_cut_point,
    should_compact,
};
use rho_sdk::contract::{MessageContent, MessageRole, ModelMessage};

#[test]
fn test_exact_bpe_token_calculation() {
    let text = "fn main() { println!(\"Hello, world!\"); }";
    let gpt4_tokens = estimate_text_tokens(text, "gpt-4");
    let claude_tokens = estimate_text_tokens(text, "claude-3-7-sonnet");

    assert!(gpt4_tokens > 0);
    assert!(claude_tokens > 0);
    assert_eq!(gpt4_tokens, claude_tokens);
}

#[test]
fn test_context_window_ceilings_and_preflight_check() {
    assert_eq!(context_window_size("gpt-5-luna"), 376_000);
    assert_eq!(context_window_size("claude-3-7-sonnet-20250219"), 200_000);
    assert_eq!(context_window_size("gemini-2.0-flash"), 1_048_576);
    assert_eq!(context_window_size("deepseek-chat"), 128_000);

    let window = context_window_size("claude-3-7-sonnet-20250219");
    let reserve = 16_384;

    assert!(!should_compact(100_000, window, reserve));
    assert!(should_compact(190_000, window, reserve));
}

#[test]
fn test_hybrid_context_tokens_with_provider_anchor() {
    let messages = vec![
        ModelMessage {
            role: MessageRole::User,
            content: vec![MessageContent::Text {
                text: "Read some files".to_string(),
            }],
        },
        ModelMessage {
            role: MessageRole::Assistant,
            content: vec![MessageContent::ToolCall {
                call_id: "c1".to_string(),
                tool_id: "tool:read".parse().unwrap(),
                arguments: serde_json::json!({"path": "src/main.rs"}),
            }],
        },
        ModelMessage {
            role: MessageRole::Tool,
            content: vec![MessageContent::ToolResult {
                call_id: "c1".to_string(),
                content: "large file content here...".to_string(),
                is_error: false,
            }],
        },
    ];

    // Anchor at message index 1 (assistant reported 1250 tokens total usage)
    let stats = calculate_context_tokens(&messages, Some((1, 1250)), "claude-3-7-sonnet");
    assert_eq!(stats.usage_anchor_tokens, 1250);
    assert!(stats.trailing_estimated_tokens > 0);
    assert_eq!(stats.total_tokens, 1250 + stats.trailing_estimated_tokens);
}

#[test]
fn test_cut_point_preserves_tool_pairs() {
    let messages = vec![
        ModelMessage {
            role: MessageRole::User,
            content: vec![MessageContent::Text {
                text: "Old instruction 1".to_string(),
            }],
        },
        ModelMessage {
            role: MessageRole::Assistant,
            content: vec![MessageContent::Text {
                text: "Old response 1".to_string(),
            }],
        },
        ModelMessage {
            role: MessageRole::User,
            content: vec![MessageContent::Text {
                text: "Middle instruction".to_string(),
            }],
        },
        ModelMessage {
            role: MessageRole::Assistant,
            content: vec![MessageContent::ToolCall {
                call_id: "c2".to_string(),
                tool_id: "tool:read".parse().unwrap(),
                arguments: serde_json::json!({"path": "lib.rs"}),
            }],
        },
        ModelMessage {
            role: MessageRole::Tool,
            content: vec![MessageContent::ToolResult {
                call_id: "c2".to_string(),
                content: "lib content".to_string(),
                is_error: false,
            }],
        },
        ModelMessage {
            role: MessageRole::User,
            content: vec![MessageContent::Text {
                text: "Recent instruction".to_string(),
            }],
        },
    ];

    // Even if keep_recent_tokens matches at index 4 (the ToolResult),
    // find_token_cut_point must back up to index 3 (the Assistant ToolCall).
    let target_tokens = estimate_message_tokens(&messages[5], "gpt-4") + estimate_message_tokens(&messages[4], "gpt-4");
    let cut_point = find_token_cut_point(&messages, target_tokens, "gpt-4");

    assert_eq!(cut_point, 3);
    assert_eq!(messages[cut_point].role, MessageRole::Assistant);
}
