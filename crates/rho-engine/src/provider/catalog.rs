//! In-memory catalog of each provider's live model list, shared by the engine and the UI.

use super::discovery::{DiscoveredModel, discover_models};
use crate::auth::AuthStore;
use crate::locking::{read, write};
use rho_harness_core::config::Config;
use rho_harness_core::provider::{Connection, ModelRoute};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};
use tokio::sync::watch;

/// Every login to a provider shares that provider's one model list.
fn catalog_key(connection: &str) -> String {
    Connection::parse(connection).provider
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderModels {
    Loading,
    Ready(Vec<DiscoveredModel>),
    Failed(String),
}

#[derive(Debug)]
struct CatalogInner {
    providers: RwLock<BTreeMap<String, ProviderModels>>,
    observed_context: RwLock<HashMap<(String, String), usize>>,
    changes: watch::Sender<u64>,
}

#[derive(Debug, Clone)]
pub struct ModelCatalog {
    inner: Arc<CatalogInner>,
}

impl Default for ModelCatalog {
    fn default() -> Self {
        Self {
            inner: Arc::new(CatalogInner {
                providers: RwLock::default(),
                observed_context: RwLock::default(),
                changes: watch::channel(0).0,
            }),
        }
    }
}

impl ModelCatalog {
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.inner.changes.subscribe()
    }

    fn notify(&self) {
        self.inner
            .changes
            .send_modify(|version| *version = version.wrapping_add(1));
    }

    pub fn set(&self, provider: &str, models: ProviderModels) {
        write(&self.inner.providers).insert(catalog_key(provider), models);
        self.notify();
    }

    pub fn get(&self, provider: &str) -> Option<ProviderModels> {
        read(&self.inner.providers).get(&catalog_key(provider)).cloned()
    }

    pub fn snapshot(&self) -> BTreeMap<String, ProviderModels> {
        read(&self.inner.providers).clone()
    }

    /// Records the window a provider actually allocated for a model, overriding its listed value.
    pub fn record_observed_context(&self, provider: &str, model: &str, tokens: usize) {
        let key = (catalog_key(provider), model.to_string());
        let previous = write(&self.inner.observed_context).insert(key, tokens);
        if previous != Some(tokens) {
            self.notify();
        }
    }

    pub fn context_tokens(&self, provider: &str, model: &str) -> Option<usize> {
        let provider = catalog_key(provider);
        let key = (provider.clone(), model.to_string());
        if let Some(tokens) = read(&self.inner.observed_context).get(&key) {
            return Some(*tokens);
        }
        match read(&self.inner.providers).get(&provider)? {
            ProviderModels::Ready(models) => models.iter().find(|m| m.id == model)?.context_tokens,
            _ => None,
        }
    }

    /// Whether `provider`'s live list contains `model` exactly. A provider only appears here once
    /// its list was fetched, so a hit also means the provider is reachable with the stored credentials.
    pub fn lists_model(&self, provider: &str, model: &str) -> bool {
        let model = model.trim();
        !model.is_empty()
            && matches!(
                read(&self.inner.providers).get(&catalog_key(provider)),
                Some(ProviderModels::Ready(models)) if models.iter().any(|m| m.id == model)
            )
    }

    pub async fn refresh(&self, provider: &str, config: &Config, auth: AuthStore) {
        self.set(provider, ProviderModels::Loading);
        let models = match discover_models(provider, config, auth).await {
            Ok(models) => ProviderModels::Ready(models),
            Err(error) => ProviderModels::Failed(error.to_string()),
        };
        self.set(provider, models);
    }

    /// Fetches one provider's list in the background, e.g. the active provider for a headless run.
    pub fn spawn_refresh(&self, provider: &str, config: &Config, auth: &AuthStore) {
        let catalog = self.clone();
        let provider = catalog_key(provider);
        let config = config.clone();
        let auth = auth.clone();
        tokio::spawn(async move { catalog.refresh(&provider, &config, auth).await });
    }

    /// Fetches every provider the user can reach (local Ollama, logged-in, and custom) in parallel,
    /// dropping providers that are no longer reachable (for example after a logout).
    pub fn spawn_refresh_all(&self, config: &Config, auth: &AuthStore) {
        let providers = reachable_providers(config, auth);
        write(&self.inner.providers).retain(|name, _| providers.contains(name));
        self.notify();
        let catalog = self.clone();
        let config = config.clone();
        let auth = auth.clone();
        tokio::spawn(async move {
            let refreshes = providers
                .iter()
                .map(|provider| catalog.refresh(provider, &config, auth.clone()));
            futures::future::join_all(refreshes).await;
        });
    }
}

pub fn reachable_providers(config: &Config, auth: &AuthStore) -> Vec<String> {
    let mut providers = vec!["local".to_string()];
    for provider in auth
        .list_configured_providers()
        .iter()
        .chain(config.providers.keys())
        .map(|name| catalog_key(name))
    {
        if !providers.contains(&provider) {
            providers.push(provider);
        }
    }
    providers
}

/// Resolves a user-entered model spec. A bare id keeps the selected provider when that provider's
/// live list actually carries it, so `deepseek-v4.1-flash` stays on `ollama-cloud` instead of being
/// routed to DeepSeek by name inference. Prefixes, explicit providers, and unreachable lists all
/// defer to `ModelRoute::parse`.
pub fn resolve_model_route(spec: &str, explicit: Option<&str>, selected: &str, catalog: &ModelCatalog) -> ModelRoute {
    let bare = spec.trim();
    if bare.is_empty() || explicit.is_some_and(|provider| !provider.trim().is_empty()) || bare.contains('/') {
        return ModelRoute::parse(bare, explicit, selected);
    }
    if catalog.lists_model(selected, bare) {
        return ModelRoute::new(selected, bare);
    }
    ModelRoute::parse(bare, None, selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_harness_core::config::ProviderConfig;
    use test_case::test_case;

    fn model(id: &str, tokens: Option<usize>) -> DiscoveredModel {
        DiscoveredModel {
            context_tokens: tokens,
            ..DiscoveredModel::new(id, id, "")
        }
    }

    #[test]
    fn context_tokens_prefer_observed_window_over_listed_one() {
        let catalog = ModelCatalog::default();
        assert_eq!(catalog.context_tokens("claude", "claude-opus-5"), None);
        catalog.set("claude", ProviderModels::Loading);
        assert_eq!(catalog.context_tokens("claude", "claude-opus-5"), None);

        catalog.set(
            "claude",
            ProviderModels::Ready(vec![model("claude-opus-5", Some(1_000_000)), model("bare", None)]),
        );
        assert_eq!(catalog.context_tokens("claude", "claude-opus-5"), Some(1_000_000));
        assert_eq!(catalog.context_tokens("claude-code", "claude-opus-5"), Some(1_000_000));
        assert_eq!(catalog.context_tokens("claude", "bare"), None);
        assert_eq!(catalog.context_tokens("anthropic", "claude-opus-5"), None);

        catalog.record_observed_context("claude", "claude-opus-5", 200_000);
        assert_eq!(catalog.context_tokens("claude", "claude-opus-5"), Some(200_000));
    }

    #[test]
    fn changes_notify_subscribers_only_when_something_changed() {
        let catalog = ModelCatalog::default();
        let mut changes = catalog.subscribe();
        catalog.set("local", ProviderModels::Failed("offline".into()));
        assert!(changes.has_changed().unwrap());
        changes.mark_unchanged();

        catalog.record_observed_context("local", "m", 4096);
        assert!(changes.has_changed().unwrap());
        changes.mark_unchanged();
        catalog.record_observed_context("local", "m", 4096);
        assert!(!changes.has_changed().unwrap());
    }

    #[tokio::test]
    async fn refresh_records_success_or_the_failure_reason() {
        let base = crate::test_support::serve_http(vec![
            ("200 OK", r#"{"data":[{"id":"acme-1"}]}"#.to_string()),
            ("503 Service Unavailable", r#"{"error":{"message":"down"}}"#.to_string()),
        ])
        .await;
        let mut config = Config::default();
        config.providers.insert(
            "acme".to_string(),
            ProviderConfig {
                base_url: base,
                ..Default::default()
            },
        );
        let (_dir, auth) = crate::test_support::temp_auth_store();
        let catalog = ModelCatalog::default();

        catalog.refresh("acme", &config, auth.clone()).await;
        let Some(ProviderModels::Ready(models)) = catalog.get("acme") else {
            panic!("expected models");
        };
        assert_eq!(models[0].id, "acme-1");

        catalog.refresh("acme", &config, auth).await;
        let Some(ProviderModels::Failed(reason)) = catalog.get("acme") else {
            panic!("expected failure");
        };
        assert!(reason.contains("HTTP 503") && reason.contains("down"), "{reason}");
    }

    #[test]
    fn reachable_providers_cover_local_logins_and_custom_endpoints_once() {
        let (_dir, mut auth) = crate::test_support::temp_auth_store();
        auth.set_key("claude", "tok").unwrap();
        auth.set_key("claude-code", "tok").unwrap();
        auth.set_key("claude:me@work.com", "tok").unwrap();
        auth.set_key("acme", "key").unwrap();
        let mut config = Config::default();
        config.providers.insert("acme".to_string(), ProviderConfig::default());
        let providers = reachable_providers(&config, &auth);
        assert_eq!(providers.first().map(String::as_str), Some("local"));
        assert_eq!(providers.iter().filter(|p| *p == "acme").count(), 1);
        assert_eq!(providers.iter().filter(|p| *p == "claude").count(), 1);
    }

    #[test]
    fn provider_aliases_and_accounts_share_one_catalog_entry() {
        let catalog = ModelCatalog::default();
        catalog.set("ollama", ProviderModels::Loading);
        assert_eq!(catalog.get("local"), Some(ProviderModels::Loading));
        catalog.set("claude:me@work.com", ProviderModels::Loading);
        assert_eq!(catalog.get("claude"), Some(ProviderModels::Loading));
        assert_eq!(catalog.snapshot().keys().collect::<Vec<_>>(), ["claude", "local"]);
    }

    fn listing(provider: &str, ids: &[&str]) -> ModelCatalog {
        let catalog = ModelCatalog::default();
        catalog.set(
            provider,
            ProviderModels::Ready(ids.iter().map(|id| DiscoveredModel::new(id, id, "")).collect()),
        );
        catalog
    }

    #[test_case("ollama-cloud", "deepseek-v4.1-flash"; "ollama cloud deepseek")]
    #[test_case("local", "gpt-oss:20b"; "local gpt-oss")]
    #[test_case("local", "deepseek-r1:7b"; "local deepseek")]
    #[test_case("openrouter", "claude-sonnet-4-6"; "openrouter claude")]
    fn bare_model_stays_on_the_provider_that_lists_it(provider: &str, model_id: &str) {
        let catalog = listing(provider, &[model_id]);
        let route = resolve_model_route(model_id, None, provider, &catalog);
        assert_eq!(route.to_string(), format!("{provider}/{model_id}"));
    }

    #[test_case(listing("ollama-cloud", &["glm-5.3-flash"]), "deepseek/deepseek-chat"; "other model listed")]
    #[test_case(ModelCatalog::default(), "deepseek/deepseek-chat"; "empty catalog")]
    fn a_provider_that_does_not_list_the_model_still_defers_to_name_inference(catalog: ModelCatalog, expected: &str) {
        let route = resolve_model_route("deepseek-chat", None, "ollama-cloud", &catalog);
        assert_eq!(route.to_string(), expected);
    }

    #[test_case("deepseek/deepseek-v4.1-flash", None, "deepseek/deepseek-v4.1-flash"; "explicit prefix")]
    #[test_case("deepseek-v4.1-flash", Some("deepseek"), "deepseek/deepseek-v4.1-flash"; "provider argument")]
    #[test_case("ollama-cloud/deepseek-v4.1-flash", None, "ollama-cloud/deepseek-v4.1-flash"; "listing prefix")]
    fn prefixes_and_explicit_providers_win_over_the_selected_provider(
        spec: &str,
        explicit: Option<&str>,
        expected: &str,
    ) {
        let catalog = listing("ollama-cloud", &["deepseek-v4.1-flash"]);
        let route = resolve_model_route(spec, explicit, "ollama-cloud", &catalog);
        assert_eq!(route.to_string(), expected);
    }

    #[test]
    fn a_listing_under_another_provider_does_not_override_the_selected_one() {
        let catalog = listing("local", &["glm-5.3-flash"]);
        let route = resolve_model_route("glm-5.3-flash", None, "ollama-cloud", &catalog);
        assert_eq!(route.provider(), "ollama-cloud");
    }
}
