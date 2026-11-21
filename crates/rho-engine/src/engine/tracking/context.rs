use crate::provider::ModelCatalog;
use rho_harness_core::model::ChatMessage;
use rho_harness_core::tokens::{ContextTokenStats, MessageTokenCache};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextLimitSource {
    Configured,
    Provider,
    Estimated,
}

#[derive(Debug, Clone, Default)]
pub struct ContextTracker {
    configured_limit: Option<usize>,
    catalog: ModelCatalog,
    probe_error: Arc<Mutex<Option<String>>>,
    token_cache: Arc<Mutex<MessageTokenCache>>,
}

impl ContextTracker {
    pub fn new(configured_limit: Option<usize>, catalog: ModelCatalog) -> Self {
        Self {
            configured_limit,
            catalog,
            probe_error: Arc::default(),
            token_cache: Arc::new(Mutex::new(MessageTokenCache::new())),
        }
    }

    pub fn resolve_limit(&self, model: &str, provider: &str) -> (usize, ContextLimitSource) {
        if let Some(limit) = self.configured_limit {
            return (limit, ContextLimitSource::Configured);
        }
        match self.catalog.context_tokens(provider, model) {
            Some(limit) => (limit, ContextLimitSource::Provider),
            None => (
                rho_harness_core::tokens::context_window_size_for_provider(model, provider),
                ContextLimitSource::Estimated,
            ),
        }
    }

    pub fn limit_for(&self, model: &str, provider: &str) -> Option<usize> {
        Some(self.resolve_limit(model, provider).0)
    }

    pub(crate) fn catalog(&self) -> &ModelCatalog {
        &self.catalog
    }

    /// Why the last attempt to read the provider's live window failed; cleared by the next success.
    pub fn probe_error(&self) -> Option<String> {
        self.probe_error.lock().unwrap().clone()
    }

    pub fn set_probe_error(&self, error: Option<String>) {
        *self.probe_error.lock().unwrap() = error;
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

#[cfg(test)]
mod tests {
    use super::{ContextLimitSource, ContextTracker};
    use crate::provider::{DiscoveredModel, ModelCatalog, ProviderModels};

    fn catalog_with(provider: &str, id: &str, tokens: usize) -> ModelCatalog {
        let catalog = ModelCatalog::default();
        let model = DiscoveredModel {
            context_tokens: Some(tokens),
            ..DiscoveredModel::new(id, id, provider, "")
        };
        catalog.set(provider, ProviderModels::Ready(vec![model]));
        catalog
    }

    #[test]
    fn limit_prefers_configured_then_provider_then_estimate() {
        let estimate = rho_harness_core::tokens::context_window_size_for_provider("m", "local");
        let cases = [
            (
                Some(65_536),
                catalog_with("local", "m", 262_144),
                (65_536, ContextLimitSource::Configured),
            ),
            (
                None,
                catalog_with("local", "m", 262_144),
                (262_144, ContextLimitSource::Provider),
            ),
            (
                None,
                catalog_with("local", "other", 262_144),
                (estimate, ContextLimitSource::Estimated),
            ),
            (None, ModelCatalog::default(), (estimate, ContextLimitSource::Estimated)),
        ];
        for (configured, catalog, expected) in cases {
            let tracker = ContextTracker::new(configured, catalog);
            assert_eq!(tracker.resolve_limit("m", "local"), expected, "{configured:?}");
        }
    }

    #[test]
    fn provider_aliases_share_one_catalog_entry() {
        let tracker = ContextTracker::new(None, catalog_with("local", "m", 8192));
        assert_eq!(tracker.limit_for("m", "ollama"), Some(8192));
    }

    #[test]
    fn catalog_updates_and_probe_errors_reach_existing_trackers() {
        let catalog = ModelCatalog::default();
        let tracker = ContextTracker::new(None, catalog.clone());
        let clone = tracker.clone();

        catalog.record_observed_context("local", "m", 262_144);
        clone.set_probe_error(Some("Ollama /api/ps request failed".to_string()));
        assert_eq!(tracker.limit_for("m", "local"), Some(262_144));
        assert_eq!(tracker.probe_error().as_deref(), Some("Ollama /api/ps request failed"));

        clone.set_probe_error(None);
        assert_eq!(tracker.probe_error(), None);
    }
}
