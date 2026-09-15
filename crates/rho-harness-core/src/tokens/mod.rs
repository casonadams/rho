use rig::message::{AssistantContent, Message, UserContent};
use std::sync::LazyLock;

pub mod cut_point;

#[cfg(test)]
mod tests;

pub use cut_point::{
    find_node_token_cut_point, find_token_cut_point, is_tool_result_message, is_user_turn_start, message_position_at,
};
pub use rho_ui_core::text::{format_size, format_tokens};

pub const ESTIMATED_IMAGE_TOKENS: usize = 1200;
pub const DEFAULT_TOKEN_OVERHEAD_PER_MESSAGE: usize = 4;
pub const DEFAULT_RESERVE_TOKENS: usize = 16_384;
pub const DEFAULT_KEEP_RECENT_TOKENS: usize = 20_000;

pub fn context_window_size_for_provider(model: &str, provider: &str) -> usize {
    rho_ui_core::modal::ModelRegistry::resolve_context_window(model, Some(provider))
}

pub fn context_window_size(model: &str) -> usize {
    rho_ui_core::modal::ModelRegistry::resolve_context_window(model, None)
}

/// Auto-compaction triggers only when context tokens exceed the window minus
/// the reserve kept for the model's response.
pub fn should_compact(context_tokens: usize, context_window: usize, reserve_tokens: usize) -> bool {
    let effective_reserve = if reserve_tokens == 0 {
        (context_window as f64 * 0.045).round() as usize
    } else {
        reserve_tokens
    };
    context_tokens > context_window.saturating_sub(effective_reserve)
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

fn estimate_user_content_tokens(item: &UserContent, model: &str) -> usize {
    match item {
        UserContent::Text(text) => estimate_text_tokens(&text.text, model),
        UserContent::ToolResult(result) => result
            .content
            .iter()
            .filter_map(|c| c.as_text())
            .map(|t| estimate_text_tokens(t, model))
            .fold(0usize, |acc, n| acc.saturating_add(n)),
        _ => 0,
    }
}

fn estimate_assistant_content_tokens(item: &AssistantContent, model: &str) -> usize {
    match item {
        AssistantContent::Text(text) => estimate_text_tokens(&text.text, model),
        AssistantContent::ToolCall(call) => {
            let name_tokens = estimate_text_tokens(&call.function.name, model);
            let args_str = call.function.arguments.to_string();
            let arg_tokens = estimate_text_tokens(&args_str, model);
            name_tokens.saturating_add(arg_tokens)
        }
        _ => 0,
    }
}

pub fn estimate_message_tokens(message: &Message, model: &str) -> usize {
    let mut tokens = DEFAULT_TOKEN_OVERHEAD_PER_MESSAGE;
    match message {
        Message::System { content } => {
            tokens = tokens.saturating_add(estimate_text_tokens(content, model));
        }
        Message::User { content } => {
            for item in content {
                tokens = tokens.saturating_add(estimate_user_content_tokens(item, model));
            }
        }
        Message::Assistant { content, .. } => {
            for item in content {
                tokens = tokens.saturating_add(estimate_assistant_content_tokens(item, model));
            }
        }
    }
    tokens
}

pub fn estimate_messages_tokens(messages: &[Message], model: &str) -> usize {
    messages.iter().map(|msg| estimate_message_tokens(msg, model)).sum()
}
