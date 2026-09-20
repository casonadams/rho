use std::collections::{HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};

use rig::message::{AssistantContent, Message, UserContent};

use super::{ContextTokenStats, estimate_message_tokens};

pub const DEFAULT_CACHE_CAPACITY: usize = 2048;

fn hash_user_content_item(item: &UserContent, hasher: &mut impl Hasher) {
    match item {
        UserContent::Text(t) => {
            0u8.hash(hasher);
            t.text.hash(hasher);
        }
        UserContent::ToolResult(r) => {
            1u8.hash(hasher);
            r.call.to_string().hash(hasher);
            r.name.hash(hasher);
            for c in &r.content {
                if let Some(txt) = c.as_text() {
                    0u8.hash(hasher);
                    txt.hash(hasher);
                }
            }
        }
        UserContent::Image(img) => {
            2u8.hash(hasher);
            match &img.data {
                rig::message::DocumentSourceKind::Raw(bytes) => {
                    0u8.hash(hasher);
                    bytes.as_slice().hash(hasher);
                }
                rig::message::DocumentSourceKind::Base64(b64) => {
                    1u8.hash(hasher);
                    b64.as_str().hash(hasher);
                }
                rig::message::DocumentSourceKind::Url(url) => {
                    2u8.hash(hasher);
                    url.as_str().hash(hasher);
                }
                rig::message::DocumentSourceKind::FileId(id) => {
                    3u8.hash(hasher);
                    id.as_str().hash(hasher);
                }
                rig::message::DocumentSourceKind::String(s) => {
                    4u8.hash(hasher);
                    s.as_str().hash(hasher);
                }
                _ => {
                    255u8.hash(hasher);
                }
            }
        }
        _ => {
            255u8.hash(hasher);
        }
    }
}

fn hash_assistant_content_item(item: &AssistantContent, hasher: &mut impl Hasher) {
    match item {
        AssistantContent::Text(t) => {
            0u8.hash(hasher);
            t.text.hash(hasher);
        }
        AssistantContent::ToolCall(c) => {
            1u8.hash(hasher);
            c.id.to_string().hash(hasher);
            c.function.name.hash(hasher);
            c.function.arguments.to_string().hash(hasher);
        }
        AssistantContent::Reasoning(r) => {
            2u8.hash(hasher);
            r.id.hash(hasher);
            for block in &r.content {
                match block {
                    rig::message::ReasoningContent::Text { text, signature } => {
                        0u8.hash(hasher);
                        text.hash(hasher);
                        signature.hash(hasher);
                    }
                    _ => 255u8.hash(hasher),
                }
            }
        }
        _ => {
            255u8.hash(hasher);
        }
    }
}

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
                hash_user_content_item(item, &mut hasher);
            }
        }
        Message::Assistant { content, .. } => {
            2u8.hash(&mut hasher);
            for item in content {
                hash_assistant_content_item(item, &mut hasher);
            }
        }
    }
    hasher.finish()
}

#[derive(Debug, Clone)]
pub struct MessageTokenCache {
    cache: HashMap<u64, usize>,
    order: VecDeque<u64>,
    capacity: usize,
    hits: usize,
    misses: usize,
}

impl Default for MessageTokenCache {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_CACHE_CAPACITY)
    }
}

impl MessageTokenCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let cap = capacity.max(1);
        Self {
            cache: HashMap::with_capacity(cap.min(256)),
            order: VecDeque::with_capacity(cap.min(256)),
            capacity: cap,
            hits: 0,
            misses: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn get_or_compute(&mut self, message: &Message, model: &str) -> usize {
        let key = hash_message(message, model);
        if let Some(&tokens) = self.cache.get(&key) {
            self.hits += 1;
            tokens
        } else {
            self.misses += 1;
            let tokens = estimate_message_tokens(message, model);
            while self.cache.len() >= self.capacity {
                if let Some(oldest) = self.order.pop_front() {
                    self.cache.remove(&oldest);
                } else {
                    break;
                }
            }
            self.cache.insert(key, tokens);
            self.order.push_back(key);
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
        self.order.clear();
        self.hits = 0;
        self.misses = 0;
    }
}
