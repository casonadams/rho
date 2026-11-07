use super::*;
use crate::model::{
    AssistantContent, ChatMessage, ImageContent, TextContent, ToolCall, ToolFunction, ToolResult, ToolResultContent,
    UserContent,
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
fn test_estimate_text_tokens_model_aware_differentiation() {
    let sample = "Exploring new frontiers 🚀 in Rust and AI: hello world!";
    let gpt4_tokens = estimate_text_tokens(sample, "gpt-4");
    let gpt4o_tokens = estimate_text_tokens(sample, "gpt-4o");
    let o1_tokens = estimate_text_tokens(sample, "o1-mini");
    let o3_tokens = estimate_text_tokens(sample, "o3-mini");

    assert!(gpt4_tokens > 0);
    assert!(gpt4o_tokens > 0);
    assert_eq!(gpt4o_tokens, o1_tokens);
    assert_eq!(gpt4o_tokens, o3_tokens);
    assert_ne!(gpt4_tokens, gpt4o_tokens);
}

#[test]
fn test_estimate_text_tokens_claude_calibration() {
    let sample = "A fast and efficient coding assistant operating inside rho, an agent harness.";
    let gpt4_tokens = estimate_text_tokens(sample, "gpt-4");
    let claude_tokens = estimate_text_tokens(sample, "claude-3-7-sonnet");

    assert!(claude_tokens >= gpt4_tokens);
    let expected = gpt4_tokens.saturating_mul(115).div_ceil(100);
    assert_eq!(claude_tokens, expected);
}

#[test]
fn test_estimate_image_tokens() {
    let msg = ChatMessage::User {
        content: vec![
            UserContent::Text(TextContent::new("Analyze this image:")),
            UserContent::Image(ImageContent {
                data: "raw_bytes".to_string(),
                media_type: None,
            }),
        ],
    };
    let tokens = estimate_message_tokens(&msg, "gpt-4o");
    let text_only_tokens = estimate_text_tokens("Analyze this image:", "gpt-4o");
    assert_eq!(
        tokens,
        text_only_tokens + ESTIMATED_IMAGE_TOKENS + DEFAULT_TOKEN_OVERHEAD_PER_MESSAGE
    );
}

#[test]
fn test_estimate_tool_result_image_tokens() {
    let unpruned_msg = ChatMessage::User {
        content: vec![UserContent::ToolResult(ToolResult {
            call: "call-1".to_string(),
            provider: None,
            name: "read".to_string(),
            content: vec![
                ToolResultContent::Text(TextContent::new("Read image file [image/png]")),
                ToolResultContent::Image(ImageContent {
                    data: "iVBORw0KGgo=".to_string(),
                    media_type: None,
                }),
            ],
        })],
    };
    let unpruned_tokens = estimate_message_tokens(&unpruned_msg, "claude-3-7-sonnet");
    let text_tokens = estimate_text_tokens("Read image file [image/png]", "claude-3-7-sonnet");
    assert_eq!(
        unpruned_tokens,
        text_tokens + ESTIMATED_IMAGE_TOKENS + DEFAULT_TOKEN_OVERHEAD_PER_MESSAGE
    );

    let pruned_msg = ChatMessage::User {
        content: vec![UserContent::ToolResult(ToolResult {
            call: "call-1".to_string(),
            provider: None,
            name: "read".to_string(),
            content: vec![ToolResultContent::Text(TextContent::new(
                "[Image 'foo.png' (image/png) read. Image content pruned for historical turn.]",
            ))],
        })],
    };
    let pruned_tokens = estimate_message_tokens(&pruned_msg, "claude-3-7-sonnet");
    assert!(unpruned_tokens >= pruned_tokens + ESTIMATED_IMAGE_TOKENS.saturating_sub(50));
}

#[test]
fn test_estimate_message_tokens() {
    let msg = ChatMessage::User {
        content: vec![
            UserContent::Text(TextContent::new("Hello world!")),
            UserContent::ToolResult(ToolResult {
                call: "call-1".to_string(),
                provider: None,
                name: "read".to_string(),
                content: vec![ToolResultContent::Text(TextContent::new("file content sample line"))],
            }),
        ],
    };

    let tokens = estimate_message_tokens(&msg, "claude-3-7-sonnet");
    assert!(tokens >= 5);
}

#[test]
fn test_calculate_context_tokens() {
    let messages = vec![
        ChatMessage::user("Initial prompt"),
        ChatMessage::assistant("Response 1"),
        ChatMessage::user("Trailing query"),
    ];

    let stats_no_anchor = calculate_context_tokens(&messages, None, "gpt-4");
    assert!(stats_no_anchor.total_tokens > 0);
    assert_eq!(stats_no_anchor.usage_anchor_tokens, 0);

    let stats_anchored = calculate_context_tokens(&messages, Some((1, 500)), "gpt-4");
    assert!(stats_anchored.total_tokens > 500);
    assert_eq!(stats_anchored.usage_anchor_tokens, 500);
}

#[test]
fn test_find_token_cut_point_and_tool_pair_preservation() {
    let messages = vec![
        ChatMessage::user("User message 1"),
        ChatMessage::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(ToolCall::new(
                "call-1",
                ToolFunction::new("read", serde_json::json!({"path": "test.txt"})),
            ))],
        },
        ChatMessage::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: "call-1".to_string(),
                provider: None,
                name: "read".to_string(),
                content: vec![ToolResultContent::Text(TextContent::new("test content"))],
            })],
        },
        ChatMessage::assistant("Assistant final"),
    ];

    let cut = find_token_cut_point(&messages, 10, "gpt-4");
    assert!(cut.cut_index <= 1);
}

#[test]
fn test_context_window_size() {
    let cases = [
        ("claude-sonnet-4-6", 1_000_000),
        ("claude-opus-4-6", 1_000_000),
        ("claude-opus-5-5", 1_000_000),
        ("claude-sonnet-5-5", 1_000_000),
        ("claude-haiku-5-5", 1_000_000),
        ("claude-haiku-5.5", 1_000_000),
        ("haiku-5.5", 1_000_000),
        ("haiku 5.5", 1_000_000),
        ("anthropic/claude-haiku-5-5", 1_000_000),
        ("anthropic/haiku-5.5", 1_000_000),
        ("claude-mythos-5.1", 1_000_000),
        ("claude-mythos-5-1", 1_000_000),
        ("claude-fable-5.1", 1_000_000),
        ("claude-fable-5-1", 1_000_000),
        ("claude-haiku-4-5", 200_000),
        ("anthropic/haiku-3.5", 200_000),
        ("o4-mini", 200_000),
        ("gpt-4o", 128_000),
        ("deepseek-chat", 128_000),
        ("qwen-2.5-coder", 128_000),
        ("gemini-2.5-pro", 2_000_000),
        ("gemini-2.5-flash", 1_000_000),
        ("gpt-6-astra", 1_050_000),
        ("gpt-6.1-sol", 372_000),
        ("gpt-6-sol", 372_000),
        ("gpt-6-terra", 372_000),
        ("gpt-6-luna", 372_000),
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

#[test]
fn context_window_size_for_gpt_6_reasoning_models_is_372k() {
    for model in ["gpt-6.1-sol", "gpt-6-sol", "gpt-6-terra", "gpt-6-luna"] {
        assert_eq!(context_window_size(model), 372_000);
        assert_eq!(context_window_size_for_provider(model, "openai"), 372_000);
        assert_eq!(context_window_size_for_provider(model, "chatgpt"), 372_000);
    }
}

#[test]
fn test_estimate_assistant_tool_call_compact_json() {
    let pretty_call = AssistantContent::ToolCall(ToolCall::new(
        "call-1",
        ToolFunction::new(
            "write",
            serde_json::Value::String("{\n  \"path\": \"hello.rs\",\n  \"content\": \"world\"\n}".to_string()),
        ),
    ));
    let compact_call = AssistantContent::ToolCall(ToolCall::new(
        "call-1",
        ToolFunction::new("write", serde_json::json!({"path": "hello.rs", "content": "world"})),
    ));
    let tokenizer = ModelTokenizer::for_model("gpt-4");
    let tokens_pretty = estimate_assistant_content_tokens_with(&pretty_call, tokenizer);
    let tokens_compact = estimate_assistant_content_tokens_with(&compact_call, tokenizer);
    assert_eq!(tokens_pretty, tokens_compact);

    let invalid_json_call = AssistantContent::ToolCall(ToolCall::new(
        "call-2",
        ToolFunction::new(
            "write",
            serde_json::Value::String("not valid json\nwith newlines".to_string()),
        ),
    ));
    assert!(estimate_assistant_content_tokens_with(&invalid_json_call, tokenizer) > 0);

    let text_assistant = AssistantContent::text("hello world");
    assert!(estimate_assistant_content_tokens_with(&text_assistant, tokenizer) > 0);

    let image_assistant = AssistantContent::Image(ImageContent {
        data: "dummy".to_string(),
        media_type: None,
    });
    assert_eq!(
        estimate_assistant_content_tokens_with(&image_assistant, tokenizer),
        ESTIMATED_IMAGE_TOKENS
    );

    let reasoning_assistant = AssistantContent::Reasoning(crate::model::Reasoning {
        id: None,
        content: vec![
            crate::model::ReasoningContent::Text {
                text: "thinking...".to_string(),
                signature: None,
            },
            crate::model::ReasoningContent::Summary("summary".to_string()),
            crate::model::ReasoningContent::Redacted {
                data: "redacted".to_string(),
            },
            crate::model::ReasoningContent::Encrypted("encrypted".to_string()),
        ],
    });
    assert!(estimate_assistant_content_tokens_with(&reasoning_assistant, tokenizer) > 0);
}

#[test]
fn test_message_token_cache_memoization() {
    let mut cache = MessageTokenCache::new();
    let messages = vec![
        ChatMessage::user("First user message"),
        ChatMessage::assistant("First assistant reply"),
        ChatMessage::user("Second user query"),
    ];

    let count1 = cache.estimate_messages_tokens_memoized(&messages, "gpt-4");
    assert_eq!(cache.misses(), 3);
    assert_eq!(cache.hits(), 0);
    assert_eq!(count1, estimate_messages_tokens(&messages, "gpt-4"));

    let count2 = cache.estimate_messages_tokens_memoized(&messages, "gpt-4");
    assert_eq!(count2, count1);
    assert_eq!(cache.misses(), 3);
    assert_eq!(cache.hits(), 3);
}

#[test]
fn test_message_token_cache_incremental_addition() {
    let mut cache = MessageTokenCache::new();
    let mut messages = vec![
        ChatMessage::user("First user message"),
        ChatMessage::assistant("First assistant reply"),
    ];

    cache.estimate_messages_tokens_memoized(&messages, "gpt-4");
    assert_eq!(cache.misses(), 2);
    assert_eq!(cache.hits(), 0);

    messages.push(ChatMessage::user("Third message newly added"));
    cache.estimate_messages_tokens_memoized(&messages, "gpt-4");
    assert_eq!(cache.misses(), 3);
    assert_eq!(cache.hits(), 2);
}

#[test]
fn test_message_token_cache_context_tokens_calculation() {
    let mut cache = MessageTokenCache::new();
    let messages = vec![
        ChatMessage::user("User question 1"),
        ChatMessage::assistant("Assistant answer 1"),
        ChatMessage::user("User question 2"),
    ];

    let stats_unanchored = cache.calculate_context_tokens(&messages, None, "gpt-4");
    let baseline_unanchored = calculate_context_tokens(&messages, None, "gpt-4");
    assert_eq!(stats_unanchored, baseline_unanchored);

    let stats_anchored = cache.calculate_context_tokens(&messages, Some((1, 400)), "gpt-4");
    let baseline_anchored = calculate_context_tokens(&messages, Some((1, 400)), "gpt-4");
    assert_eq!(stats_anchored, baseline_anchored);
}

#[test]
fn test_message_token_cache_clear() {
    let mut cache = MessageTokenCache::new();
    let msg = ChatMessage::user("test message");
    cache.get_or_compute(&msg, "gpt-4");
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.misses(), 1);

    cache.clear();
    assert!(cache.is_empty());
    assert_eq!(cache.hits(), 0);
    assert_eq!(cache.misses(), 0);
}

#[test]
fn test_hash_message_image_efficiency() {
    let msg1 = ChatMessage::User {
        content: vec![UserContent::Image(ImageContent {
            data: "raw_bytes_1".to_string(),
            media_type: None,
        })],
    };
    let msg2 = ChatMessage::User {
        content: vec![UserContent::Image(ImageContent {
            data: "raw_bytes_1".to_string(),
            media_type: None,
        })],
    };
    let msg3 = ChatMessage::User {
        content: vec![UserContent::Image(ImageContent {
            data: "raw_bytes_2".to_string(),
            media_type: None,
        })],
    };
    let msg4 = ChatMessage::User {
        content: vec![UserContent::Image(ImageContent {
            data: "raw_bytes_1".to_string(),
            media_type: Some("image/png".to_string()),
        })],
    };

    let hash1 = hash_message(&msg1, "gpt-4o");
    let hash2 = hash_message(&msg2, "gpt-4o");
    let hash3 = hash_message(&msg3, "gpt-4o");
    let hash4 = hash_message(&msg4, "gpt-4o");

    assert_eq!(hash1, hash2);
    assert_ne!(hash1, hash3);
    assert_ne!(hash1, hash4);
}

#[test]
fn test_hash_message_tool_result_image() {
    let msg1 = ChatMessage::User {
        content: vec![UserContent::ToolResult(ToolResult {
            call: "call-1".to_string(),
            provider: None,
            name: "read".to_string(),
            content: vec![
                ToolResultContent::Text(TextContent::new("Read image")),
                ToolResultContent::Image(ImageContent {
                    data: "data1".to_string(),
                    media_type: None,
                }),
            ],
        })],
    };
    let msg2 = ChatMessage::User {
        content: vec![UserContent::ToolResult(ToolResult {
            call: "call-1".to_string(),
            provider: None,
            name: "read".to_string(),
            content: vec![
                ToolResultContent::Text(TextContent::new("Read image")),
                ToolResultContent::Image(ImageContent {
                    data: "data1".to_string(),
                    media_type: None,
                }),
            ],
        })],
    };
    let msg3 = ChatMessage::User {
        content: vec![UserContent::ToolResult(ToolResult {
            call: "call-1".to_string(),
            provider: None,
            name: "read".to_string(),
            content: vec![
                ToolResultContent::Text(TextContent::new("Read image")),
                ToolResultContent::Image(ImageContent {
                    data: "data2".to_string(),
                    media_type: None,
                }),
            ],
        })],
    };

    assert_eq!(hash_message(&msg1, "gpt-4o"), hash_message(&msg2, "gpt-4o"));
    assert_ne!(hash_message(&msg1, "gpt-4o"), hash_message(&msg3, "gpt-4o"));
}

#[test]
fn test_message_token_cache_bounded() {
    let mut cache = MessageTokenCache::with_capacity(3);
    assert_eq!(cache.capacity(), 3);

    let m1 = ChatMessage::user("message 1");
    let m2 = ChatMessage::user("message 2");
    let m3 = ChatMessage::user("message 3");
    let m4 = ChatMessage::user("message 4");

    cache.get_or_compute(&m1, "gpt-4o");
    cache.get_or_compute(&m2, "gpt-4o");
    cache.get_or_compute(&m3, "gpt-4o");
    assert_eq!(cache.len(), 3);

    cache.get_or_compute(&m4, "gpt-4o");
    assert_eq!(cache.len(), 3);

    let hits_before = cache.hits();
    cache.get_or_compute(&m2, "gpt-4o");
    cache.get_or_compute(&m3, "gpt-4o");
    cache.get_or_compute(&m4, "gpt-4o");
    assert_eq!(cache.hits(), hits_before + 3);

    let misses_before = cache.misses();
    cache.get_or_compute(&m1, "gpt-4o");
    assert_eq!(cache.misses(), misses_before + 1);
    assert_eq!(cache.len(), 3);
}

#[test]
fn context_window_matches_model_id_tokens_not_substrings() {
    assert_eq!(context_window_size("terraform-plan"), 128_000);
    assert_eq!(context_window_size("console-mini"), 128_000);
    assert_eq!(context_window_size("solar-pro"), 128_000);
    assert_eq!(context_window_size("luna"), 372_000);
    assert_eq!(context_window_size("gpt-6.1-sol"), 372_000);
    assert_eq!(context_window_size("gpt-6-terra-mini"), 372_000);
    assert_eq!(context_window_size("qwen2.5-coder:7b"), 128_000);
    assert_eq!(context_window_size("gemini-2.5-pro-preview"), 2_000_000);
}

#[test]
fn o200k_tokenizer_selection_uses_model_id_tokens() {
    assert!(is_o200k_model("gpt-4o-mini"));
    assert!(is_o200k_model("gpt-6-terra"));
    assert!(!is_o200k_model("terraform-plan"));
    assert!(!is_o200k_model("console-mini"));
}
