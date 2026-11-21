//! Model picker entries built from the live, in-memory provider catalog.

use super::completion::ModelItem;
use rho_engine::provider::discovery::{DiscoveredModel, format_context_tokens, sort_models_newest_first};
use rho_engine::provider::{ModelCatalog, ProviderModels};
use rho_harness_core::config::Config;
use rho_harness_core::provider::Connection;
use std::sync::Arc;

fn push_unique(models: &mut Vec<ModelItem>, item: ModelItem) {
    if !models.iter().any(|m| m.id == item.id && m.provider == item.provider) {
        models.push(item);
    }
}

fn active_model_item(config: &Config, catalog: &ModelCatalog) -> ModelItem {
    let route = config.route();
    let window = catalog
        .context_tokens(route.provider(), &route.model)
        .unwrap_or_else(|| rho_harness_core::tokens::context_window_size(&route.model));
    ModelItem {
        id: route.model.clone(),
        provider: route.connection.to_string(),
        description: format!("{} · active", format_context_tokens(window)),
    }
}

fn listed_item(connection: &str, model: &DiscoveredModel) -> ModelItem {
    ModelItem {
        id: model.id.clone(),
        provider: connection.to_string(),
        description: model.description.clone(),
    }
}

/// The logins `provider`'s models are offered under: each stored account login, plus the bare
/// provider unless it is reachable only through accounts (no default login stored).
fn connections_of(provider: &str, logins: &[Connection]) -> Vec<String> {
    let stored: Vec<&Connection> = logins.iter().filter(|login| login.provider == provider).collect();
    let only_accounts = !stored.is_empty() && stored.iter().all(|login| login.account.is_some());
    let bare = (!only_accounts).then(|| provider.to_string());
    bare.into_iter()
        .chain(
            stored
                .iter()
                .filter(|login| login.account.is_some())
                .map(ToString::to_string),
        )
        .collect()
}

/// Active model first (when one is selected), then every model the providers listed: local Ollama,
/// then the rest by name. Each listed model is offered once per login to its provider.
pub fn discover_models(config: &Config, catalog: &ModelCatalog, logins: &[Connection]) -> Vec<ModelItem> {
    let mut models = Vec::new();
    if !config.model.trim().is_empty() {
        models.push(active_model_item(config, catalog));
    }
    let mut providers: Vec<(String, ProviderModels)> = catalog.snapshot().into_iter().collect();
    providers.sort_by_key(|(name, _)| name != "local");
    for (provider, state) in providers {
        if let ProviderModels::Ready(listed) = state {
            let listed = sort_models_newest_first(listed);
            for connection in connections_of(&provider, logins) {
                for model in &listed {
                    push_unique(&mut models, listed_item(&connection, model));
                }
            }
        }
    }
    models
}

/// One line describing providers still fetching or that failed; empty when every list is in.
/// A missing local Ollama only matters when it is the active provider.
pub fn catalog_status(config: &Config, catalog: &ModelCatalog) -> String {
    let local_active = config.route().provider() == "local";
    let mut loading = Vec::new();
    let mut failed = Vec::new();
    for (provider, state) in catalog.snapshot() {
        match state {
            ProviderModels::Loading => loading.push(provider),
            ProviderModels::Failed(_) if provider == "local" && !local_active => {}
            ProviderModels::Failed(reason) => failed.push(format!("{provider}: {reason}")),
            ProviderModels::Ready(_) => {}
        }
    }
    let mut parts = Vec::new();
    if !loading.is_empty() {
        parts.push(format!("Fetching models from {}…", loading.join(", ")));
    }
    if !failed.is_empty() {
        parts.push(format!("Could not list models for {}", failed.join("; ")));
    }
    parts.join("\n")
}

/// Model completions that read the catalog at completion time, so they fill in as fetches finish.
#[derive(Clone)]
pub struct ModelList(Arc<dyn Fn() -> Vec<ModelItem> + Send + Sync>);

impl ModelList {
    pub fn fixed(models: Vec<ModelItem>) -> Self {
        Self(Arc::new(move || models.clone()))
    }

    pub fn live(config: Config, catalog: ModelCatalog, logins: Vec<Connection>) -> Self {
        Self(Arc::new(move || discover_models(&config, &catalog, &logins)))
    }

    pub fn items(&self) -> Vec<ModelItem> {
        (self.0)()
    }
}

impl Default for ModelList {
    fn default() -> Self {
        Self::fixed(Vec::new())
    }
}

impl std::fmt::Debug for ModelList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ModelList").field(&self.items()).finish()
    }
}

#[cfg(test)]
pub(crate) fn catalog_with(provider: &str, ids: &[&str]) -> ModelCatalog {
    let catalog = ModelCatalog::default();
    let models = ids.iter().map(|id| DiscoveredModel::new(id, id, "listed")).collect();
    catalog.set(provider, ProviderModels::Ready(models));
    catalog
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(id: &str, tokens: Option<usize>) -> DiscoveredModel {
        DiscoveredModel {
            context_tokens: tokens,
            description: tokens.map(format_context_tokens).unwrap_or_default(),
            ..DiscoveredModel::new(id, id, "")
        }
    }

    fn summarize(models: &[ModelItem]) -> Vec<(&str, &str, &str)> {
        models
            .iter()
            .map(|m| (m.id.as_str(), m.provider.as_str(), m.description.as_str()))
            .collect()
    }

    fn config(provider: &str, model: &str) -> Config {
        Config {
            provider: provider.to_string(),
            model: model.to_string(),
            ..Config::default()
        }
    }

    #[test]
    fn models_list_active_first_then_local_then_ready_providers() {
        let catalog = ModelCatalog::default();
        catalog.set(
            "claude",
            ProviderModels::Ready(vec![
                listed("claude-opus-4-6", Some(1_000_000)),
                listed("claude-opus-5", Some(1_000_000)),
            ]),
        );
        catalog.set("local", ProviderModels::Ready(vec![listed("qwen:7b", Some(32_768))]));
        catalog.set("chatgpt", ProviderModels::Loading);
        catalog.set("gemini", ProviderModels::Failed("HTTP 401".into()));

        let models = discover_models(&config("claude", "claude-opus-5"), &catalog, &[]);
        assert_eq!(
            summarize(&models),
            vec![
                ("claude-opus-5", "claude", "1M ctx · active"),
                ("qwen:7b", "local", "32k ctx"),
                ("claude-opus-4-6", "claude", "1M ctx"),
            ]
        );
    }

    #[test]
    fn each_login_offers_its_providers_models() {
        let catalog = ModelCatalog::default();
        catalog.set("claude", ProviderModels::Ready(vec![listed("claude-opus-5", None)]));
        catalog.set("chatgpt", ProviderModels::Ready(vec![listed("gpt-6", None)]));
        let both = ["claude", "claude:me@work.com"].map(Connection::parse);
        let work_only = [Connection::parse("claude:me@work.com")];
        let cases: [(&[Connection], Vec<&str>); 2] = [
            (&both, vec!["claude:me@work.com", "chatgpt", "claude"]),
            (&work_only, vec!["claude:me@work.com", "chatgpt"]),
        ];
        for (logins, expected) in cases {
            let models = discover_models(&config("claude:me@work.com", "claude-opus-5"), &catalog, logins);
            let providers: Vec<&str> = models.iter().map(|m| m.provider.as_str()).collect();
            assert_eq!(providers, expected, "{logins:?}");
            assert_eq!(models[0].canonical_id(), "claude:me@work.com/claude-opus-5");
        }
    }

    #[test]
    fn active_model_without_a_listed_window_shows_the_estimate() {
        let models = discover_models(&config("openai", "gpt-4o"), &ModelCatalog::default(), &[]);
        assert_eq!(summarize(&models), vec![("gpt-4o", "openai", "128k ctx · active")]);
    }

    #[test]
    fn no_selected_model_lists_only_provider_models() {
        let catalog = ModelCatalog::default();
        assert!(discover_models(&Config::default(), &catalog, &[]).is_empty());
        catalog.set("local", ProviderModels::Ready(vec![listed("qwen:7b", None)]));
        assert_eq!(
            summarize(&discover_models(&Config::default(), &catalog, &[])),
            vec![("qwen:7b", "local", "")]
        );
    }

    #[test]
    fn status_reports_loading_and_failures_but_hides_absent_local_ollama() {
        let catalog = ModelCatalog::default();
        assert_eq!(catalog_status(&config("claude", "m"), &catalog), "");

        catalog.set("chatgpt", ProviderModels::Loading);
        catalog.set("gemini", ProviderModels::Failed("HTTP 401".into()));
        catalog.set("local", ProviderModels::Failed("connection refused".into()));
        assert_eq!(
            catalog_status(&config("claude", "m"), &catalog),
            "Fetching models from chatgpt…\nCould not list models for gemini: HTTP 401"
        );
        assert!(catalog_status(&config("local", "m"), &catalog).contains("local: connection refused"));
    }

    #[test]
    fn live_model_list_reflects_catalog_changes() {
        let catalog = ModelCatalog::default();
        let list = ModelList::live(config("claude", "claude-opus-5"), catalog.clone(), Vec::new());
        assert_eq!(list.items().len(), 1);
        catalog.set("claude", ProviderModels::Ready(vec![listed("claude-sonnet-5", None)]));
        assert_eq!(list.items().len(), 2);
    }
}
