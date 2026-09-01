use rho::tokens::{
    calculate_context_tokens, context_window_size, estimate_text_tokens, find_token_cut_point, should_compact,
};
use rig::message::{
    AssistantContent, Message, ToolCall, ToolCallId, ToolFunction, ToolResult, ToolResultContent, UserContent,
};

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
        Message::user("Read some files"),
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
                content: vec![ToolResultContent::Text(rig::message::Text::new(
                    "large file content here...",
                ))],
            })],
        },
    ];

    let stats = calculate_context_tokens(&messages, Some((1, 1250)), "claude-3-7-sonnet");
    assert_eq!(stats.usage_anchor_tokens, 1250);
    assert!(stats.trailing_estimated_tokens > 0);
    assert_eq!(stats.total_tokens, 1250 + stats.trailing_estimated_tokens);
}

#[test]
fn test_cut_point_preserves_tool_pairs() {
    let messages = vec![
        Message::user("User prompt 1"),
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
                content: vec![ToolResultContent::Text(rig::message::Text::new("file content here"))],
            })],
        },
        Message::assistant("Assistant summary"),
    ];

    let cut_idx = find_token_cut_point(&messages, 10, "gpt-4");
    assert!(cut_idx <= 1);
}
