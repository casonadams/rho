use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use rig::message::{AssistantContent, Message, UserContent};

use super::{ContextTokenStats, estimate_message_tokens};

pub fn hash_message(message: &Message, model: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    model.hash(&mut hasher);
    match message {
        Message::System { content } => {
            0u8.hash(&mut hasher);
            content.hash(&mut hasher);
        }
        Message::User { content } => {
            1u8.hash(&mut hasher);
            for item in content {
                match item {
                    UserContent::Text(t) => {
                        0u8.hash(&mut hasher);
                        t.text.hash(&mut hasher);
                    }
                    UserContent::ToolResult(r) => {
                        1u8.hash(&mut hasher);
                        r.call.to_string().hash(&mut hasher);
                        r.name.hash(&mut hasher);
                        for c in &r.content {
                            if let Some(txt) = c.as_text() {
                                0u8.hash(&mut hasher);
                                txt.hash(&mut hasher);
                            }
                        }
                    }
                    UserContent::Image(img) => {
                        2u8.hash(&mut hasher);
                        format!("{:?}", img.data).hash(&mut hasher);
                    }
                    _ => {
                        255u8.hash(&mut hasher);
                    }
                }
            }
        }
        Message::Assistant { content, .. } => {
            2u8.hash(&mut hasher);
            for item in content {
                match item {
                    AssistantContent::Text(t) => {
                        0u8.hash(&mut hasher);
                        t.text.hash(&mut hasher);
                    }
                    AssistantContent::ToolCall(c) => {
                        1u8.hash(&mut hasher);
                        c.id.to_string().hash(&mut hasher);
                        c.function.name.hash(&mut hasher);
                        c.function.arguments.to_string().hash(&mut hasher);
                    }
                    AssistantContent::Reasoning(r) => {
                        2u8.hash(&mut hasher);
                        format!("{:?}", r).hash(&mut hasher);
                    }
                    _ => {
                        255u8.hash(&mut hasher);
                    }
                }
            }
        }
    }
    hasher.finish()
}

#[derive(Debug, Clone, Default)]
pub struct MessageTokenCache {
    cache: HashMap<u64, usize>,
    hits: usize,
    misses: usize,
}

impl MessageTokenCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_compute(&mut self, message: &Message, model: &str) -> usize {
        let key = hash_message(message, model);
        if let Some(&tokens) = self.cache.get(&key) {
            self.hits += 1;
            tokens
        } else {
            self.misses += 1;
            let tokens = estimate_message_tokens(message, model);
            self.cache.insert(key, tokens);
            tokens
        }
    }

    pub fn estimate_messages_tokens_memoized(&mut self, messages: &[Message], model: &str) -> usize {
        messages.iter().map(|msg| self.get_or_compute(msg, model)).sum()
    }

    pub fn calculate_context_tokens(
        &mut self,
        messages: &[Message],
        last_usage_anchor: Option<(usize, usize)>,
        model: &str,
    ) -> ContextTokenStats {
        if let Some((anchor_idx, anchor_tokens)) = last_usage_anchor
            && anchor_idx < messages.len()
        {
            let trailing_estimated = self.estimate_messages_tokens_memoized(&messages[anchor_idx + 1..], model);
            ContextTokenStats {
                total_tokens: anchor_tokens.saturating_add(trailing_estimated),
                usage_anchor_tokens: anchor_tokens,
                trailing_estimated_tokens: trailing_estimated,
            }
        } else {
            let estimated = self.estimate_messages_tokens_memoized(messages, model);
            ContextTokenStats {
                total_tokens: estimated,
                usage_anchor_tokens: 0,
                trailing_estimated_tokens: estimated,
            }
        }
    }

    pub fn hits(&self) -> usize {
        self.hits
    }

    pub fn misses(&self) -> usize {
        self.misses
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    pub fn clear(&mut self) {
        self.cache.clear();
        self.hits = 0;
        self.misses = 0;
    }
}
