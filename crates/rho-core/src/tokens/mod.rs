use rho_sdk::contract::{MessageContent, ModelMessage};
use std::sync::LazyLock;

#[cfg(test)]
mod tests;

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
