use rho_harness_core::model::ChatMessage;
use rho_harness_core::tokens::{ContextTokenStats, MessageTokenCache};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Default)]
pub struct ContextTracker {
    configured_limit: Option<usize>,
    token_cache: Arc<Mutex<MessageTokenCache>>,
}

impl ContextTracker {
    pub fn new(configured_limit: Option<usize>) -> Self {
        Self {
            configured_limit,
            token_cache: Arc::new(Mutex::new(MessageTokenCache::new())),
        }
    }

    pub fn limit_for(&self, model: &str, provider: &str) -> Option<usize> {
        if let Some(limit) = self.configured_limit {
            return Some(limit);
        }
        Some(rho_harness_core::tokens::context_window_size_for_provider(
            model, provider,
        ))
    }

    pub fn estimate_messages_tokens(&self, messages: &[ChatMessage], model: &str) -> usize {
        self.token_cache
            .lock()
            .unwrap()
            .estimate_messages_tokens_memoized(messages, model)
    }

    pub fn estimate_each_message_tokens(&self, messages: &[ChatMessage], model: &str) -> Vec<usize> {
        let mut cache = self.token_cache.lock().unwrap();
        messages.iter().map(|msg| cache.get_or_compute(msg, model)).collect()
    }

    pub fn calculate_context_tokens(
        &self,
        messages: &[ChatMessage],
        last_usage_anchor: Option<(usize, usize)>,
        model: &str,
    ) -> ContextTokenStats {
        self.token_cache
            .lock()
            .unwrap()
            .calculate_context_tokens(messages, last_usage_anchor, model)
    }
}
