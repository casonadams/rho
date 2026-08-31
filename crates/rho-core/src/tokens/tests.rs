use super::*;
use rho_sdk::contract::MessageRole;

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
    let msg = ModelMessage {
        role: MessageRole::User,
        content: vec![
            MessageContent::Text {
                text: "Hello world!".to_string(),
            },
            MessageContent::ToolResult {
                call_id: "call-1".to_string(),
                content: "file content sample line".to_string(),
                is_error: false,
            },
        ],
    };

    let tokens = estimate_message_tokens(&msg, "claude-3-7-sonnet");
    assert!(tokens >= 5);
}

#[test]
fn test_calculate_context_tokens_and_should_compact() {
    let messages = vec![
        ModelMessage {
            role: MessageRole::User,
            content: vec![MessageContent::Text {
                text: "Initial prompt".to_string(),
            }],
        },
        ModelMessage {
            role: MessageRole::Assistant,
            content: vec![MessageContent::Text {
                text: "Response 1".to_string(),
            }],
        },
        ModelMessage {
            role: MessageRole::User,
            content: vec![MessageContent::Text {
                text: "Trailing query".to_string(),
            }],
        },
    ];

    // Without anchor
    let stats_no_anchor = calculate_context_tokens(&messages, None, "gpt-4");
    assert!(stats_no_anchor.total_tokens > 0);
    assert_eq!(stats_no_anchor.usage_anchor_tokens, 0);

    // With anchor at index 1 (reported 500 tokens)
    let stats_anchored = calculate_context_tokens(&messages, Some((1, 500)), "gpt-4");
    assert!(stats_anchored.total_tokens > 500);
    assert_eq!(stats_anchored.usage_anchor_tokens, 500);

    // should_compact
    let window = 200_000;
    let reserve = 16_384;
    assert!(!should_compact(50_000, window, reserve));
    assert!(should_compact(190_000, window, reserve));
}

#[test]
fn test_find_token_cut_point_and_tool_pair_preservation() {
    let messages = vec![
        ModelMessage {
            role: MessageRole::User,
            content: vec![MessageContent::Text {
                text: "User message 1".to_string(),
            }],
        },
        ModelMessage {
            role: MessageRole::Assistant,
            content: vec![MessageContent::ToolCall {
                call_id: "call-1".to_string(),
                tool_id: "tool:read".parse().unwrap(),
                arguments: serde_json::json!({"path":"test.txt"}),
            }],
        },
        ModelMessage {
            role: MessageRole::Tool,
            content: vec![MessageContent::ToolResult {
                call_id: "call-1".to_string(),
                content: "test content".to_string(),
                is_error: false,
            }],
        },
        ModelMessage {
            role: MessageRole::User,
            content: vec![MessageContent::Text {
                text: "User message 2".to_string(),
            }],
        },
    ];

    // If keep_recent_tokens is small enough that cut would land on index 2 (ToolResult),
    // it must back up to index 1 (Assistant ToolCall) so the tool call is not separated!
    let cut_idx = find_token_cut_point(&messages, 15, "gpt-4");
    assert_eq!(cut_idx, 1);
    assert_ne!(messages[cut_idx].role, MessageRole::Tool);
}
