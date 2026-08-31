use rho_sdk::contract::{MessageContent, ModelMessage};

pub const ESTIMATED_IMAGE_TOKENS: usize = 1200;
pub const DEFAULT_TOKEN_OVERHEAD_PER_MESSAGE: usize = 4;
pub const DEFAULT_RESERVE_TOKENS: usize = 16_384;
pub const DEFAULT_KEEP_RECENT_TOKENS: usize = 20_000;

pub fn context_window_size(model: &str) -> usize {
    let lower = model.to_lowercase();
    if lower.contains("gemini") {
        1_048_576
    } else if lower.contains("gpt-5") || lower.contains("luna") || lower.contains("codex") {
        376_000
    } else if lower.contains("claude-3-7") || lower.contains("claude-3-5") || lower.contains("claude-3") {
        200_000
    } else {
        128_000
    }
}

pub fn should_compact(context_tokens: usize, context_window: usize, reserve_tokens: usize) -> bool {
    context_tokens > context_window.saturating_sub(reserve_tokens)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextTokenStats {
    pub total_tokens: usize,
    pub usage_anchor_tokens: usize,
    pub trailing_estimated_tokens: usize,
}

pub fn calculate_context_tokens(
    messages: &[ModelMessage],
    last_usage_anchor: Option<(usize, usize)>, // (index_in_messages, reported_total_tokens)
    model: &str,
) -> ContextTokenStats {
    if let Some((anchor_idx, anchor_tokens)) = last_usage_anchor
        && anchor_idx < messages.len()
    {
        let trailing_estimated = estimate_messages_tokens(&messages[anchor_idx + 1..], model);
        ContextTokenStats {
            total_tokens: anchor_tokens.saturating_add(trailing_estimated),
            usage_anchor_tokens: anchor_tokens,
            trailing_estimated_tokens: trailing_estimated,
        }
    } else {
        let estimated = estimate_messages_tokens(messages, model);
        ContextTokenStats {
            total_tokens: estimated,
            usage_anchor_tokens: 0,
            trailing_estimated_tokens: estimated,
        }
    }
}

pub fn find_token_cut_point(messages: &[ModelMessage], keep_recent_tokens: usize, model: &str) -> usize {
    if messages.is_empty() {
        return 0;
    }
    let mut accumulated_tokens: usize = 0;
    let mut cut_idx = messages.len();

    for i in (0..messages.len()).rev() {
        let msg_tokens = estimate_message_tokens(&messages[i], model);
        accumulated_tokens = accumulated_tokens.saturating_add(msg_tokens);
        cut_idx = i;
        if accumulated_tokens >= keep_recent_tokens {
            break;
        }
    }

    // Never start the kept slice with a Tool result without its Assistant caller
    while cut_idx > 0 && messages[cut_idx].role == rho_sdk::contract::MessageRole::Tool {
        cut_idx -= 1;
    }

    cut_idx
}

use std::sync::LazyLock;

static CL100K_BPE: LazyLock<Option<tiktoken_rs::CoreBPE>> = LazyLock::new(|| tiktoken_rs::cl100k_base().ok());

pub fn estimate_text_tokens(text: &str, _model: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    if let Some(bpe) = CL100K_BPE.as_ref() {
        return bpe.encode_with_special_tokens(text).len();
    }
    estimate_char_tokens(text)
}

pub fn estimate_char_tokens(text: &str) -> usize {
    let chars = text.chars().count();
    chars.div_ceil(4)
}

pub fn estimate_message_tokens(message: &ModelMessage, model: &str) -> usize {
    let mut tokens = DEFAULT_TOKEN_OVERHEAD_PER_MESSAGE;
    for content in &message.content {
        match content {
            MessageContent::Text { text } => {
                tokens = tokens.saturating_add(estimate_text_tokens(text, model));
            }
            MessageContent::ToolCall { tool_id, arguments, .. } => {
                tokens = tokens.saturating_add(estimate_text_tokens(tool_id.name(), model));
                let args_str = arguments.to_string();
                tokens = tokens.saturating_add(estimate_text_tokens(&args_str, model));
            }
            MessageContent::ToolResult { content, .. } => {
                tokens = tokens.saturating_add(estimate_text_tokens(content, model));
            }
        }
    }
    tokens
}

pub fn estimate_messages_tokens(messages: &[ModelMessage], model: &str) -> usize {
    messages.iter().map(|msg| estimate_message_tokens(msg, model)).sum()
}

#[cfg(test)]
mod tests {
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
}
