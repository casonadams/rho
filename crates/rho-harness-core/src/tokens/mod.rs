use rig::message::{AssistantContent, Message, UserContent};
use std::sync::LazyLock;

#[cfg(test)]
mod tests;

pub const ESTIMATED_IMAGE_TOKENS: usize = 1200;
pub const DEFAULT_TOKEN_OVERHEAD_PER_MESSAGE: usize = 4;
pub const DEFAULT_RESERVE_TOKENS: usize = 16_384;
pub const DEFAULT_KEEP_RECENT_TOKENS: usize = 20_000;

pub fn context_window_size(model: &str) -> usize {
    let lower = model.to_lowercase();
    if lower.contains("gemini-1.5-pro") || lower.contains("gemini-2.5-pro") {
        2_000_000
    } else if lower.contains("gemini") {
        1_000_000
    } else if lower.contains("gpt-5.6") || lower.contains("luna") || lower.contains("terra") || lower.contains("sol") {
        372_000
    } else if lower.contains("gpt-5.4") || lower.contains("gpt-5.5") {
        272_000
    } else if lower.contains("claude") || lower.contains("o1") || lower.contains("o3") {
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
    messages: &[Message],
    last_usage_anchor: Option<(usize, usize)>,
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

fn is_tool_result_message(message: &Message) -> bool {
    if let Message::User { content } = message {
        content.iter().any(|c| matches!(c, UserContent::ToolResult(_)))
    } else {
        false
    }
}

pub fn find_token_cut_point(messages: &[Message], keep_recent_tokens: usize, model: &str) -> usize {
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

    while cut_idx > 0 && is_tool_result_message(&messages[cut_idx]) {
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

pub fn estimate_message_tokens(message: &Message, model: &str) -> usize {
    let mut tokens = DEFAULT_TOKEN_OVERHEAD_PER_MESSAGE;
    match message {
        Message::System { content } => {
            tokens = tokens.saturating_add(estimate_text_tokens(content, model));
        }
        Message::User { content } => {
            for item in content {
                match item {
                    UserContent::Text(text) => {
                        tokens = tokens.saturating_add(estimate_text_tokens(&text.text, model));
                    }
                    UserContent::ToolResult(result) => {
                        for c in &result.content {
                            if let Some(t) = c.as_text() {
                                tokens = tokens.saturating_add(estimate_text_tokens(t, model));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Message::Assistant { content, .. } => {
            for item in content {
                match item {
                    AssistantContent::Text(text) => {
                        tokens = tokens.saturating_add(estimate_text_tokens(&text.text, model));
                    }
                    AssistantContent::ToolCall(call) => {
                        tokens = tokens.saturating_add(estimate_text_tokens(&call.function.name, model));
                        let args_str = call.function.arguments.to_string();
                        tokens = tokens.saturating_add(estimate_text_tokens(&args_str, model));
                    }
                    _ => {}
                }
            }
        }
    }
    tokens
}

pub fn estimate_messages_tokens(messages: &[Message], model: &str) -> usize {
    messages.iter().map(|msg| estimate_message_tokens(msg, model)).sum()
}
