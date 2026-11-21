//! In-memory catalog of each provider's live model list, shared by the engine and the UI.

use super::discovery::{DiscoveredModel, discover_models};
use crate::auth::AuthStore;
use rho_harness_core::config::Config;
use rho_harness_core::provider::canonical_provider;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, LazyLock, RwLock};
use tokio::sync::watch;

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

static SHARED: LazyLock<ModelCatalog> = LazyLock::new(ModelCatalog::default);

impl ModelCatalog {
    /// The process-wide catalog; every engine and UI surface in one rho process reads the same lists.
    pub fn shared() -> Self {
        SHARED.clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.inner.changes.subscribe()
    }

    fn notify(&self) {
        self.inner
            .changes
            .send_modify(|version| *version = version.wrapping_add(1));
    }

    pub fn set(&self, provider: &str, models: ProviderModels) {
        self.inner
            .providers
            .write()
            .unwrap()
            .insert(canonical_provider(provider), models);
        self.notify();
    }

    pub fn get(&self, provider: &str) -> Option<ProviderModels> {
        self.inner
            .providers
            .read()
            .unwrap()
            .get(&canonical_provider(provider))
            .cloned()
    }

    pub fn snapshot(&self) -> BTreeMap<String, ProviderModels> {
        self.inner.providers.read().unwrap().clone()
    }

    /// Records the window a provider actually allocated for a model, overriding its listed value.
    pub fn record_observed_context(&self, provider: &str, model: &str, tokens: usize) {
        let key = (canonical_provider(provider), model.to_string());
        let previous = self.inner.observed_context.write().unwrap().insert(key, tokens);
        if previous != Some(tokens) {
            self.notify();
        }
    }

    pub fn context_tokens(&self, provider: &str, model: &str) -> Option<usize> {
        let provider = canonical_provider(provider);
        let key = (provider.clone(), model.to_string());
        if let Some(tokens) = self.inner.observed_context.read().unwrap().get(&key) {
            return Some(*tokens);
        }
        match self.inner.providers.read().unwrap().get(&provider)? {
            ProviderModels::Ready(models) => models.iter().find(|m| m.id == model)?.context_tokens,
            _ => None,
        }
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
        let provider = canonical_provider(provider);
        let config = config.clone();
        let auth = auth.clone();
        tokio::spawn(async move { catalog.refresh(&provider, &config, auth).await });
    }

    /// Fetches every provider the user can reach (local Ollama, logged-in, and custom) in parallel,
    /// dropping providers that are no longer reachable (for example after a logout).
    pub fn spawn_refresh_all(&self, config: &Config, auth: &AuthStore) {
        let providers = reachable_providers(config, auth);
        self.inner
            .providers
            .write()
            .unwrap()
            .retain(|name, _| providers.contains(name));
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
        .map(|name| canonical_provider(name))
    {
        if !providers.contains(&provider) {
            providers.push(provider);
        }
    }
    providers
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_harness_core::config::ProviderConfig;

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
        auth.set_key("acme", "key").unwrap();
        let mut config = Config::default();
        config.providers.insert("acme".to_string(), ProviderConfig::default());
        let providers = reachable_providers(&config, &auth);
        assert_eq!(providers.first().map(String::as_str), Some("local"));
        assert_eq!(providers.iter().filter(|p| *p == "acme").count(), 1);
        assert_eq!(providers.iter().filter(|p| *p == "claude").count(), 1);
    }

    #[test]
    fn provider_aliases_share_one_catalog_entry() {
        let catalog = ModelCatalog::default();
        catalog.set("ollama", ProviderModels::Loading);
        assert_eq!(catalog.get("local"), Some(ProviderModels::Loading));
        assert_eq!(catalog.snapshot().keys().collect::<Vec<_>>(), ["local"]);
    }
}
