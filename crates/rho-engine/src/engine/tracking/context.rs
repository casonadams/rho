use crate::engine::metrics::format_tokens;
use crate::provider::ModelCatalog;
use rho_harness_core::model::ChatMessage;
use rho_harness_core::provider::ModelRoute;
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
    probe_error: Arc<Mutex<Option<String>>>,
    token_cache: Arc<Mutex<MessageTokenCache>>,
    notified_percent: Arc<Mutex<Option<u64>>>,
}

/// Context usage, in percent of the window and highest first, at which the model is told how full it is.
const NOTICE_PERCENTS: [u64; 3] = [90, 75, 50];

/// The highest notice percentage that `used` tokens reach in a `limit`-token window, if it is above
/// `notified`, the last one reported. A zero `limit` is an unknown window and never reaches one.
fn crossed_notice_percent(used: u64, limit: u64, notified: Option<u64>) -> Option<u64> {
    if limit == 0 {
        return None;
    }
    let percent = used.saturating_mul(100) / limit;
    NOTICE_PERCENTS
        .into_iter()
        .find(|&p| percent >= p)
        .filter(|&p| notified.is_none_or(|n| p > n))
}

/// The window a turn's requests fill, and where auto-compaction starts; tells the model, once per
/// notice percentage crossed since the last compaction, how full it is.
#[derive(Debug, Clone)]
pub struct ContextBudget {
    limit: u64,
    compaction_at: u64,
    notified_percent: Arc<Mutex<Option<u64>>>,
}

impl ContextBudget {
    /// The notice for a request whose prompt used `used` tokens, when it crossed a new notice percentage.
    pub fn notice(&self, used: u64) -> Option<String> {
        let mut notified = self.notified_percent.lock().unwrap();
        *notified = Some(crossed_notice_percent(used, self.limit, *notified)?);
        Some(format!(
            "[context: {} of {} tokens used ({}%); auto-compaction at {}]",
            format_tokens(used),
            format_tokens(self.limit),
            used.saturating_mul(100) / self.limit,
            format_tokens(self.compaction_at)
        ))
    }
}

impl ContextTracker {
    pub fn new(configured_limit: Option<usize>) -> Self {
        Self {
            configured_limit,
            probe_error: Arc::default(),
            token_cache: Arc::new(Mutex::new(MessageTokenCache::new())),
            notified_percent: Arc::default(),
        }
    }

    /// The budget for a turn against a `limit`-token window that auto-compacts above `compaction_at`.
    pub fn budget(&self, limit: usize, compaction_at: usize) -> ContextBudget {
        ContextBudget {
            limit: limit as u64,
            compaction_at: compaction_at as u64,
            notified_percent: Arc::clone(&self.notified_percent),
        }
    }

    /// Starts the notices over for a freshly compacted context.
    pub fn reset_notices(&self) {
        *self.notified_percent.lock().unwrap() = None;
    }

    /// The configured limit, else the provider's window for `route`, else an estimate from its name.
    pub fn resolve_limit(&self, catalog: &ModelCatalog, route: &ModelRoute) -> (usize, ContextLimitSource) {
        if let Some(limit) = self.configured_limit {
            return (limit, ContextLimitSource::Configured);
        }
        match catalog.context_tokens(route.provider(), &route.model) {
            Some(limit) => (limit, ContextLimitSource::Provider),
            None => (
                rho_harness_core::tokens::context_window_size_for_provider(&route.model, route.provider()),
                ContextLimitSource::Estimated,
            ),
        }
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
    use rho_harness_core::provider::ModelRoute;

    fn catalog_with(provider: &str, id: &str, tokens: usize) -> ModelCatalog {
        let catalog = ModelCatalog::default();
        let model = DiscoveredModel {
            context_tokens: Some(tokens),
            ..DiscoveredModel::new(id, id, "")
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
            let tracker = ContextTracker::new(configured);
            assert_eq!(
                tracker.resolve_limit(&catalog, &ModelRoute::new("local", "m")),
                expected,
                "{configured:?}"
            );
        }
    }

    #[test]
    fn provider_aliases_share_one_catalog_entry() {
        let tracker = ContextTracker::new(None);
        let catalog = catalog_with("local", "m", 8192);
        assert_eq!(tracker.resolve_limit(&catalog, &ModelRoute::new("ollama", "m")).0, 8192);
    }

    #[test]
    fn notice_percent_is_the_highest_crossed_above_the_last_reported() {
        let cases = [
            ("below the first", 99, 200, None, None),
            ("first crossing", 100, 200, None, Some(50)),
            ("several at once report the highest", 190, 200, None, Some(90)),
            ("same percentage does not repeat", 140, 200, Some(50), None),
            ("next crossing", 150, 200, Some(50), Some(75)),
            ("dropping below does not repeat", 60, 200, Some(75), None),
            ("past the window", 300, 200, Some(75), Some(90)),
            ("unknown window", 190, 0, None, None),
        ];
        for (case, used, limit, notified, expected) in cases {
            assert_eq!(super::crossed_notice_percent(used, limit, notified), expected, "{case}");
        }
    }

    #[test]
    fn budget_notices_each_crossing_once_until_compaction() {
        let tracker = ContextTracker::new(None);
        let budget = tracker.budget(200_000, 183_616);
        let notices = |used: &[u64]| -> Vec<Option<String>> { used.iter().map(|&u| budget.notice(u)).collect() };
        assert_eq!(
            notices(&[90_000, 142_000, 145_000, 160_000]),
            [
                None,
                Some("[context: 142k of 200k tokens used (71%); auto-compaction at 184k]".to_string()),
                None,
                Some("[context: 160k of 200k tokens used (80%); auto-compaction at 184k]".to_string()),
            ]
        );
        tracker.reset_notices();
        assert_eq!(
            tracker.budget(200_000, 183_616).notice(100_000).as_deref(),
            Some("[context: 100k of 200k tokens used (50%); auto-compaction at 184k]")
        );
    }

    #[test]
    fn probe_errors_reach_existing_trackers() {
        let tracker = ContextTracker::new(None);
        let clone = tracker.clone();
        clone.set_probe_error(Some("Ollama /api/ps request failed".to_string()));
        assert_eq!(tracker.probe_error().as_deref(), Some("Ollama /api/ps request failed"));
        clone.set_probe_error(None);
        assert_eq!(tracker.probe_error(), None);
    }
}
