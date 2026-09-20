use rig::message::{AssistantContent, Message, UserContent};
use std::sync::LazyLock;

pub mod cut_point;
pub mod memo;

#[cfg(test)]
mod tests;

pub use cut_point::{
    find_node_token_cut_point, find_token_cut_point, is_tool_result_message, is_user_turn_start, message_position_at,
};
pub use memo::{DEFAULT_CACHE_CAPACITY, MessageTokenCache, hash_message};

pub const ESTIMATED_IMAGE_TOKENS: usize = 1200;
pub const DEFAULT_TOKEN_OVERHEAD_PER_MESSAGE: usize = 4;
pub const DEFAULT_RESERVE_TOKENS: usize = 16_384;
pub const DEFAULT_KEEP_RECENT_TOKENS: usize = 20_000;

const MODEL_CONTEXT_WINDOWS: &[(&[&str], usize)] = &[
    (&["gemini-1.5-pro", "gemini-2.5-pro"], 2_000_000),
    (&["gemini"], 1_000_000),
    (&["gpt-6-astra"], 1_050_000),
    (&["sonnet", "opus", "fable"], 1_000_000),
    (&["gpt-5.6", "luna", "terra", "sol"], 372_000),
    (&["gpt-5.4", "gpt-5.5"], 272_000),
    (&["claude", "o1", "o3"], 200_000),
];

pub fn context_window_size_for_provider(model: &str, provider: &str) -> usize {
    if model.eq_ignore_ascii_case("gpt-6-astra") {
        return if provider.eq_ignore_ascii_case("openai") {
            1_050_000
        } else if provider.eq_ignore_ascii_case("chatgpt") {
            372_000
        } else {
            128_000
        };
    }
    context_window_size(model)
}

pub fn context_window_size(model: &str) -> usize {
    let lower = model.to_lowercase();
    for &(patterns, window) in MODEL_CONTEXT_WINDOWS {
        if patterns.iter().any(|&p| lower.contains(p)) {
            return window;
        }
    }
    128_000
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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
static O200K_BPE: LazyLock<Option<tiktoken_rs::CoreBPE>> = LazyLock::new(|| tiktoken_rs::o200k_base().ok());

fn is_o200k_model(lower: &str) -> bool {
    if lower.contains("gpt-4o") || lower.contains("gpt-5") || lower.contains("gpt-6") {
        return true;
    }
    lower
        .split(['/', ':', '_'])
        .any(|segment| segment.starts_with("o1") || segment.starts_with("o3"))
}

fn bpe_for_model(model: &str) -> Option<&'static tiktoken_rs::CoreBPE> {
    let lower = model.to_lowercase();
    if is_o200k_model(&lower) {
        O200K_BPE.as_ref().or(CL100K_BPE.as_ref())
    } else {
        CL100K_BPE.as_ref().or(O200K_BPE.as_ref())
    }
}

pub fn estimate_text_tokens(text: &str, model: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    if let Some(bpe) = bpe_for_model(model) {
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
        UserContent::Image(_) => ESTIMATED_IMAGE_TOKENS,
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
            let args_str = match &call.function.arguments {
                serde_json::Value::String(s) => {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(s) {
                        serde_json::to_string(&val).unwrap_or_else(|_| s.clone())
                    } else {
                        s.clone()
                    }
                }
                other => serde_json::to_string(other).unwrap_or_else(|_| other.to_string()),
            };
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

#[derive(Debug, Clone, Default)]
pub struct BpeTokenCounter {
    model: String,
}

impl BpeTokenCounter {
    pub fn new(model: impl Into<String>) -> Self {
        Self { model: model.into() }
    }
}

impl rig_memory::TokenCounter for BpeTokenCounter {
    fn count(&self, message: &Message) -> usize {
        estimate_message_tokens(message, &self.model)
    }
}
