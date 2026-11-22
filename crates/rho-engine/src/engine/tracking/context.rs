use crate::engine::metrics::format_tokens;
use crate::provider::ModelCatalog;
use rho_harness_core::model::ChatMessage;
use rho_harness_core::provider::ModelRoute;
use rho_harness_core::tokens::{ContextTokenStats, MessageTokenCache};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

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
/// notice percentage crossed since the last compaction, how full it is. A window that is only an
/// estimate gives no notices.
#[derive(Debug, Clone)]
pub struct ContextBudget {
    limit: u64,
    compaction_at: u64,
    announce: bool,
    notified_percent: Arc<Mutex<Option<u64>>>,
}

impl ContextBudget {
    /// The notice for a request whose prompt used `used` tokens, when it crossed a new notice percentage.
    /// The crossing counts as reported only once `deliver` accepts the notice.
    pub fn notice(&self, used: u64, deliver: impl FnOnce(&str) -> bool) {
        if !self.announce {
            return;
        }
        let notified = *lock(&self.notified_percent);
        let Some(percent) = crossed_notice_percent(used, self.limit, notified) else {
            return;
        };
        let notice = format!(
            "[context: {} of {} tokens used ({}%); compaction starts near {} at the next turn]",
            format_tokens(used),
            format_tokens(self.limit),
            used.saturating_mul(100) / self.limit,
            format_tokens(self.compaction_at)
        );
        if deliver(&notice) {
            *lock(&self.notified_percent) = Some(percent);
        }
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
    pub fn budget(&self, limit: usize, source: ContextLimitSource, compaction_at: usize) -> ContextBudget {
        ContextBudget {
            limit: limit as u64,
            compaction_at: compaction_at as u64,
            announce: source != ContextLimitSource::Estimated,
            notified_percent: Arc::clone(&self.notified_percent),
        }
    }

    /// Starts the notices over for a freshly compacted or switched context.
    pub fn reset_notices(&self) {
        *lock(&self.notified_percent) = None;
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
        lock(&self.probe_error).clone()
    }

    pub fn set_probe_error(&self, error: Option<String>) {
        *lock(&self.probe_error) = error;
    }

    pub fn estimate_messages_tokens(&self, messages: &[ChatMessage], model: &str) -> usize {
        lock(&self.token_cache).estimate_messages_tokens_memoized(messages, model)
    }

    pub fn estimate_each_message_tokens(&self, messages: &[ChatMessage], model: &str) -> Vec<usize> {
        let mut cache = lock(&self.token_cache);
        messages.iter().map(|msg| cache.get_or_compute(msg, model)).collect()
    }

    pub fn calculate_context_tokens(
        &self,
        messages: &[ChatMessage],
        last_usage_anchor: Option<(usize, usize)>,
        model: &str,
    ) -> ContextTokenStats {
        lock(&self.token_cache).calculate_context_tokens(messages, last_usage_anchor, model)
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

    fn delivered(budget: &super::ContextBudget, used: u64, accept: bool) -> Option<String> {
        let mut seen = None;
        budget.notice(used, |notice| {
            seen = Some(notice.to_string());
            accept
        });
        seen
    }

    #[test]
    fn budget_notices_each_crossing_once_until_compaction() {
        let tracker = ContextTracker::new(None);
        let budget = tracker.budget(200_000, ContextLimitSource::Provider, 183_616);
        let notices =
            |used: &[u64]| -> Vec<Option<String>> { used.iter().map(|&u| delivered(&budget, u, true)).collect() };
        assert_eq!(
            notices(&[90_000, 142_000, 145_000, 160_000]),
            [
                None,
                Some(
                    "[context: 142k of 200k tokens used (71%); compaction starts near 184k at the next turn]"
                        .to_string()
                ),
                None,
                Some(
                    "[context: 160k of 200k tokens used (80%); compaction starts near 184k at the next turn]"
                        .to_string()
                ),
            ]
        );
        tracker.reset_notices();
        assert_eq!(
            delivered(
                &tracker.budget(200_000, ContextLimitSource::Provider, 183_616),
                100_000,
                true
            )
            .as_deref(),
            Some("[context: 100k of 200k tokens used (50%); compaction starts near 184k at the next turn]")
        );
    }

    #[test]
    fn a_notice_that_could_not_be_delivered_is_offered_again() {
        let budget = ContextTracker::new(None).budget(200_000, ContextLimitSource::Provider, 183_616);
        assert!(delivered(&budget, 120_000, false).is_some());
        assert!(delivered(&budget, 121_000, true).is_some());
        assert_eq!(delivered(&budget, 122_000, true), None);
    }

    #[test]
    fn estimated_windows_give_no_notices() {
        let budget = ContextTracker::new(None).budget(200_000, ContextLimitSource::Estimated, 183_616);
        assert_eq!(delivered(&budget, 190_000, true), None);
    }

    #[test]
    fn a_poisoned_lock_does_not_stop_notices_or_reset() {
        let tracker = ContextTracker::new(None);
        let poisoner = tracker.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.notified_percent.lock().unwrap();
            panic!("poison");
        })
        .join();
        let budget = tracker.budget(200_000, ContextLimitSource::Provider, 183_616);
        assert!(delivered(&budget, 120_000, true).is_some());
        tracker.reset_notices();
        assert!(delivered(&budget, 120_000, true).is_some());
    }

    #[test]
    fn the_lock_is_not_held_while_delivering() {
        let budget = ContextTracker::new(None).budget(200_000, ContextLimitSource::Provider, 183_616);
        let mut nested = None;
        budget.notice(120_000, |_| {
            nested = delivered(&budget, 130_000, true);
            true
        });
        assert!(nested.is_some());
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
