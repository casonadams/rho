use std::collections::{HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};

use crate::model::{AssistantContent, ChatMessage, ImageContent, ReasoningContent, ToolResultContent, UserContent};

use super::{ContextTokenStats, estimate_message_tokens};

pub const DEFAULT_CACHE_CAPACITY: usize = 2048;

fn hash_image_content(img: &ImageContent, hasher: &mut impl Hasher) {
    img.data.hash(hasher);
    img.media_type.hash(hasher);
}

fn hash_tool_result_content(c: &ToolResultContent, hasher: &mut impl Hasher) {
    match c {
        ToolResultContent::Text(t) => {
            0u8.hash(hasher);
            t.text.hash(hasher);
        }
        ToolResultContent::Image(img) => {
            1u8.hash(hasher);
            hash_image_content(img, hasher);
        }
        ToolResultContent::Json { value } => {
            2u8.hash(hasher);
            value.to_string().hash(hasher);
        }
    }
}

fn hash_reasoning_content(block: &ReasoningContent, hasher: &mut impl Hasher) {
    match block {
        ReasoningContent::Text { text, signature } => {
            0u8.hash(hasher);
            text.hash(hasher);
            signature.hash(hasher);
        }
        ReasoningContent::Summary(s) => {
            1u8.hash(hasher);
            s.hash(hasher);
        }
        ReasoningContent::Redacted { data } => {
            2u8.hash(hasher);
            data.hash(hasher);
        }
        ReasoningContent::Encrypted(e) => {
            3u8.hash(hasher);
            e.hash(hasher);
        }
    }
}

fn hash_user_content_item(item: &UserContent, hasher: &mut impl Hasher) {
    match item {
        UserContent::Text(t) => {
            0u8.hash(hasher);
            t.text.hash(hasher);
        }
        UserContent::ToolResult(r) => {
            1u8.hash(hasher);
            r.call.hash(hasher);
            r.name.hash(hasher);
            for c in &r.content {
                hash_tool_result_content(c, hasher);
            }
        }
        UserContent::Image(img) => {
            2u8.hash(hasher);
            hash_image_content(img, hasher);
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
            c.id.hash(hasher);
            c.function.name.hash(hasher);
            c.function.arguments.to_string().hash(hasher);
        }
        AssistantContent::Reasoning(r) => {
            2u8.hash(hasher);
            r.id.hash(hasher);
            for block in &r.content {
                hash_reasoning_content(block, hasher);
            }
        }
        AssistantContent::Image(img) => {
            3u8.hash(hasher);
            hash_image_content(img, hasher);
        }
    }
}

pub fn hash_message(message: &ChatMessage, model: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    model.hash(&mut hasher);
    match message {
        ChatMessage::System { content } => {
            0u8.hash(&mut hasher);
            content.hash(&mut hasher);
        }
        ChatMessage::User { content } => {
            1u8.hash(&mut hasher);
            for item in content {
                hash_user_content_item(item, &mut hasher);
            }
        }
        ChatMessage::Assistant { content, .. } => {
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
        Self {
            cache: HashMap::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
            capacity: capacity.max(1),
            hits: 0,
            misses: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    fn insert(&mut self, key: u64, tokens: usize) {
        if self.cache.len() >= self.capacity {
            while let Some(oldest) = self.order.pop_front() {
                if self.cache.remove(&oldest).is_some() {
                    break;
                }
            }
        }
        self.cache.insert(key, tokens);
        self.order.push_back(key);
    }

    pub fn get_or_compute(&mut self, message: &ChatMessage, model: &str) -> usize {
        let key = hash_message(message, model);
        if let Some(&tokens) = self.cache.get(&key) {
            self.hits += 1;
            tokens
        } else {
            self.misses += 1;
            let tokens = estimate_message_tokens(message, model);
            self.insert(key, tokens);
            tokens
        }
    }

    pub fn estimate_messages_tokens_memoized(&mut self, messages: &[ChatMessage], model: &str) -> usize {
        messages.iter().map(|msg| self.get_or_compute(msg, model)).sum()
    }

    pub fn calculate_context_tokens(
        &mut self,
        messages: &[ChatMessage],
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
