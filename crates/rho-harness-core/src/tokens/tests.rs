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
fn test_estimate_image_tokens() {
    let msg = Message::User {
        content: vec![
            UserContent::text("Analyze this image:"),
            UserContent::image_raw(vec![1, 2, 3, 4], None, None),
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

#[test]
fn test_bpe_token_counter_matches_estimate_message_tokens() {
    use rig_memory::TokenCounter;

    let model = "gpt-4";
    let counter = BpeTokenCounter::new(model);

    let messages = [
        Message::system("System instructions for testing"),
        Message::user("Hello user prompt"),
        Message::assistant("Hello assistant response"),
        Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(ToolCall::new(
                ToolCallId::new_or_mint("call-1"),
                ToolFunction::new("read".to_string(), serde_json::json!({"path": "file.txt"})),
            ))],
        },
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: ToolCallId::new_or_mint("call-1"),
                provider: None,
                name: "read".to_string(),
                content: vec![ToolResultContent::Text(rig::message::Text::new("file contents"))],
            })],
        },
    ];

    for msg in &messages {
        let expected = estimate_message_tokens(msg, model);
        assert_eq!(counter.count(msg), expected);
    }

    let default_counter = BpeTokenCounter::default();
    let text_msg = Message::user("Another test message");
    assert_eq!(default_counter.count(&text_msg), estimate_message_tokens(&text_msg, ""));

    let _policy = rig_memory::TokenWindowMemory::new(1000, counter);
}

#[test]
fn test_estimate_assistant_tool_call_compact_json() {
    let pretty_call = AssistantContent::ToolCall(ToolCall::new(
        ToolCallId::new_or_mint("call-1"),
        ToolFunction::new(
            "write".to_string(),
            serde_json::Value::String("{\n  \"path\": \"hello.rs\",\n  \"content\": \"world\"\n}".to_string()),
        ),
    ));
    let compact_call = AssistantContent::ToolCall(ToolCall::new(
        ToolCallId::new_or_mint("call-1"),
        ToolFunction::new(
            "write".to_string(),
            serde_json::json!({"path": "hello.rs", "content": "world"}),
        ),
    ));
    let tokens_pretty = estimate_assistant_content_tokens(&pretty_call, "gpt-4");
    let tokens_compact = estimate_assistant_content_tokens(&compact_call, "gpt-4");
    assert_eq!(tokens_pretty, tokens_compact);
}

#[test]
fn test_message_token_cache_memoization() {
    let mut cache = MessageTokenCache::new();
    let messages = vec![
        Message::user("First user message"),
        Message::assistant("First assistant reply"),
        Message::user("Second user query"),
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
        Message::user("First user message"),
        Message::assistant("First assistant reply"),
    ];

    cache.estimate_messages_tokens_memoized(&messages, "gpt-4");
    assert_eq!(cache.misses(), 2);
    assert_eq!(cache.hits(), 0);

    // Add 1 message
    messages.push(Message::user("Third message newly added"));
    cache.estimate_messages_tokens_memoized(&messages, "gpt-4");
    // Prior 2 messages were hits, 1 new message was a miss
    assert_eq!(cache.misses(), 3);
    assert_eq!(cache.hits(), 2);
}

#[test]
fn test_message_token_cache_context_tokens_calculation() {
    let mut cache = MessageTokenCache::new();
    let messages = vec![
        Message::user("User question 1"),
        Message::assistant("Assistant answer 1"),
        Message::user("User question 2"),
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
    let msg = Message::user("test message");
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
    let msg1 = Message::User {
        content: vec![UserContent::image_raw(vec![1, 2, 3, 4], None, None)],
    };
    let msg2 = Message::User {
        content: vec![UserContent::image_raw(vec![1, 2, 3, 4], None, None)],
    };
    let msg3 = Message::User {
        content: vec![UserContent::image_raw(vec![1, 2, 3, 5], None, None)],
    };
    let msg4 = Message::User {
        content: vec![UserContent::image_base64("aGVsbG8=", None, None)],
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
fn test_message_token_cache_bounded() {
    let mut cache = MessageTokenCache::with_capacity(3);
    assert_eq!(cache.capacity(), 3);

    let m1 = Message::user("message 1");
    let m2 = Message::user("message 2");
    let m3 = Message::user("message 3");
    let m4 = Message::user("message 4");

    cache.get_or_compute(&m1, "gpt-4o");
    cache.get_or_compute(&m2, "gpt-4o");
    cache.get_or_compute(&m3, "gpt-4o");
    assert_eq!(cache.len(), 3);

    // Adding 4th message should evict oldest (m1)
    cache.get_or_compute(&m4, "gpt-4o");
    assert_eq!(cache.len(), 3);

    // m2, m3, m4 should be hits
    let hits_before = cache.hits();
    cache.get_or_compute(&m2, "gpt-4o");
    cache.get_or_compute(&m3, "gpt-4o");
    cache.get_or_compute(&m4, "gpt-4o");
    assert_eq!(cache.hits(), hits_before + 3);

    // m1 was evicted, so accessing it should miss
    let misses_before = cache.misses();
    cache.get_or_compute(&m1, "gpt-4o");
    assert_eq!(cache.misses(), misses_before + 1);
    assert_eq!(cache.len(), 3);
}
