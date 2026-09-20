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
    let gpt4o_tokens = estimate_text_tokens(text, "gpt-4o");

    assert!(gpt4_tokens > 0);
    assert!(claude_tokens > 0);
    assert!(gpt4o_tokens > 0);

    let rich_text = "Exploring new frontiers 🚀 in Rust and AI: hello world!";
    let gpt4_rich = estimate_text_tokens(rich_text, "gpt-4");
    let gpt4o_rich = estimate_text_tokens(rich_text, "gpt-4o");
    let o1_rich = estimate_text_tokens(rich_text, "o1-mini");
    let o3_rich = estimate_text_tokens(rich_text, "o3-mini");

    assert_eq!(gpt4o_rich, o1_rich);
    assert_eq!(gpt4o_rich, o3_rich);
    assert_ne!(gpt4_rich, gpt4o_rich);
}

#[test]
fn test_context_window_ceilings() {
    let cases = [
        ("gpt-5-luna", 372_000),
        ("claude-3-7-sonnet-20250219", 1_000_000),
        ("claude-3-haiku-20240307", 200_000),
        ("gemini-2.0-flash", 1_000_000),
        ("deepseek-chat", 128_000),
    ];
    for (model, expected) in cases {
        assert_eq!(context_window_size(model), expected);
    }
}

#[test]
fn test_preflight_check_compaction() {
    let window = context_window_size("claude-3-haiku-20240307");
    assert!(!should_compact(100_000, window, 0) && !should_compact(190_000, window, 0));
    assert!(should_compact(192_000, window, 0) && should_compact(195_000, window, 0));
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

    let cut = find_token_cut_point(&messages, 10, "gpt-4");
    assert!(cut.cut_index <= 1);
}

#[test]
fn test_tool_result_image_token_estimation() {
    use rho::tokens::{ESTIMATED_IMAGE_TOKENS, estimate_message_tokens};

    let msg = Message::User {
        content: vec![UserContent::ToolResult(ToolResult {
            call: ToolCallId::new_or_mint("c1"),
            provider: None,
            name: "read".to_string(),
            content: vec![
                ToolResultContent::Text(rig::message::Text::new("Read image file")),
                ToolResultContent::image_base64("data", None, None),
            ],
        })],
    };
    let tokens = estimate_message_tokens(&msg, "claude-3-7-sonnet");
    assert!(tokens >= ESTIMATED_IMAGE_TOKENS);
}

#[test]
fn test_multi_turn_image_tool_result_pruning_token_reduction() {
    use rho::engine::runner::{DEFAULT_PRUNE_LINE_THRESHOLD, prune_historical_tool_outputs};
    use rho::tokens::{ESTIMATED_IMAGE_TOKENS, estimate_message_tokens};

    let turn1_user = Message::user("Inspect this mock screenshot");
    let turn1_call = Message::Assistant {
        id: None,
        content: vec![AssistantContent::ToolCall(ToolCall::new(
            ToolCallId::new_or_mint("c1"),
            ToolFunction::new(
                "read".to_string(),
                serde_json::json!({ "path": "docs/architecture.png" }),
            ),
        ))],
    };
    let turn1_result = Message::User {
        content: vec![UserContent::ToolResult(ToolResult {
            call: ToolCallId::new_or_mint("c1"),
            provider: None,
            name: "read".to_string(),
            content: vec![
                ToolResultContent::Text(rig::message::Text::new(
                    "Read image file [image/png]\n[Image: 1200x800]",
                )),
                ToolResultContent::image_base64(
                    "iVBORw0KGgoAAAANSUhEUg==",
                    Some(rig::completion::message::ImageMediaType::PNG),
                    None,
                ),
            ],
        })],
    };
    let turn1_assistant =
        Message::assistant("The diagram shows three main architectural layers: core, engine, and shell.");
    let turn2_user = Message::user("Now explain the responsibilities of the shell layer");

    let history = vec![turn1_user, turn1_call, turn1_result, turn1_assistant, turn2_user];

    let pre_tokens: usize = history
        .iter()
        .map(|m| estimate_message_tokens(m, "claude-3-7-sonnet"))
        .sum();
    assert!(pre_tokens > ESTIMATED_IMAGE_TOKENS);

    let pruned_history = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    assert_eq!(pruned_history.len(), history.len());

    let Message::User {
        content: pruned_content,
    } = &pruned_history[2]
    else {
        panic!("expected user message with tool result");
    };
    assert_eq!(pruned_content.len(), 1);
    let UserContent::ToolResult(pruned_result) = &pruned_content[0] else {
        panic!("expected tool result");
    };
    assert_eq!(pruned_result.content.len(), 1);
    assert!(matches!(&pruned_result.content[0], ToolResultContent::Text(_)));
    let text = pruned_result.content[0].as_text().unwrap();
    assert_eq!(
        text,
        "[Image 'docs/architecture.png' (image/png) read. Image content pruned for historical turn.]"
    );

    let post_tokens: usize = pruned_history
        .iter()
        .map(|m| estimate_message_tokens(m, "claude-3-7-sonnet"))
        .sum();
    let stub_tokens = estimate_text_tokens(text, "claude-3-7-sonnet");
    assert!(pre_tokens >= post_tokens + ESTIMATED_IMAGE_TOKENS.saturating_sub(stub_tokens));
}
