//! Dynamic model discovery and capability descriptors for autocomplete and model switcher.

use super::completion::ModelItem;
use rho_engine::auth::AuthStore;
use rho_engine::provider::discovery::discover_provider_models;
use rho_engine::provider::store::ModelStore;
use rho_harness_core::config::Config;
use rho_harness_core::provider::ProviderId;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::str::FromStr;

fn push_model_item_unique(models: &mut Vec<ModelItem>, item: ModelItem) {
    if !models
        .iter()
        .any(|existing| existing.id == item.id && existing.provider == item.provider)
    {
        models.push(item);
    }
}

fn append_active_model(models: &mut Vec<ModelItem>, config: &Config, store: &ModelStore) {
    let active_ctx = store
        .get_models(&config.provider)
        .and_then(|m_list| m_list.iter().find(|m| m.id == config.model))
        .and_then(|m| m.context_tokens)
        .unwrap_or_else(|| rho_harness_core::tokens::context_window_size(&config.model));
    let ctx_desc = rho_engine::provider::discovery::format_context_tokens(active_ctx);
    models.push(ModelItem {
        id: config.model.clone(),
        provider: config.provider.clone(),
        description: format!("{ctx_desc} · active"),
    });
}

fn append_local_models(models: &mut Vec<ModelItem>, store: &ModelStore) {
    let local = store
        .get_models("local")
        .or_else(|| store.get_models("ollama"))
        .cloned()
        .unwrap_or_else(|| rho_engine::provider::discovery::default_presets_for("local"));
    let local = rho_engine::provider::discovery::sort_models_newest_first(local);
    for m in local {
        push_model_item_unique(
            models,
            ModelItem {
                id: m.id,
                provider: m.provider,
                description: m.description,
            },
        );
    }
}

fn merge_provider_presets(
    prov: &str,
    mut models: Vec<rho_engine::provider::discovery::DiscoveredModel>,
) -> Vec<rho_engine::provider::discovery::DiscoveredModel> {
    for preset in rho_engine::provider::discovery::default_presets_for(prov) {
        if !models.iter().any(|m| m.id == preset.id) {
            models.push(preset);
        }
    }
    rho_engine::provider::discovery::sort_models_newest_first(models)
}

fn append_configured_provider_models(models: &mut Vec<ModelItem>, store: &ModelStore, auth_store: &AuthStore) {
    let mut configured: Vec<String> = auth_store.list_configured_providers();
    for prov in store.providers() {
        if !configured.contains(prov) {
            configured.push(prov.clone());
        }
    }
    for prov in configured.iter().filter(|p| *p != "local" && *p != "ollama") {
        let raw_models = store.get_models(prov).cloned().unwrap_or_default();
        let prov_models = merge_provider_presets(prov, raw_models);
        for m in prov_models {
            push_model_item_unique(
                models,
                ModelItem {
                    id: m.id,
                    provider: m.provider,
                    description: m.description,
                },
            );
        }
    }
}

fn append_custom_provider_models(models: &mut Vec<ModelItem>, config: &Config, store: &ModelStore) {
    for (name, spec) in &config.providers {
        if let Some(cached) = store.get_models(name) {
            let cached = rho_engine::provider::discovery::sort_models_newest_first(cached.clone());
            for m in cached {
                push_model_item_unique(
                    models,
                    ModelItem {
                        id: m.id.clone(),
                        provider: m.provider.clone(),
                        description: m.description.clone(),
                    },
                );
            }
        } else if name != &config.provider {
            models.push(ModelItem {
                id: format!("{name}-default"),
                provider: name.clone(),
                description: format!("endpoint: {}", spec.base_url),
            });
        }
    }
}

/// Dynamically discovers models available to the current user from active configuration,
/// local Ollama, live/cached provider discovery catalogs, and custom endpoints.
pub fn discover_models(config: &Config, auth_store: &AuthStore) -> Vec<ModelItem> {
    let mut models = Vec::new();
    let model_store = ModelStore::load(config.config_dir.join("models-store.json"));
    append_active_model(&mut models, config, &model_store);
    append_local_models(&mut models, &model_store);
    append_configured_provider_models(&mut models, &model_store, auth_store);
    append_custom_provider_models(&mut models, config, &model_store);
    models
}

async fn refresh_single_auth_provider(store: &mut ModelStore, auth: &AuthStore, prov: &str) {
    if let Ok(id) = ProviderId::from_str(prov)
        && let Ok(discovered) = discover_provider_models(id, auth).await
    {
        let _ = store.set_models_async(prov, discovered).await;
    }
}

fn is_remote_provider(p: &str) -> bool {
    p != "local" && p != "ollama"
}

async fn refresh_auth_provider_models(store: &mut ModelStore, auth: &AuthStore) {
    for prov in auth.list_configured_providers() {
        if is_remote_provider(&prov) {
            refresh_single_auth_provider(store, auth, &prov).await;
        }
    }
}

async fn discover_custom(
    name: &str,
    base_url: &str,
    auth: &AuthStore,
) -> Option<Vec<rho_engine::provider::discovery::DiscoveredModel>> {
    let key = auth.get_key_sync(name).ok().flatten();
    rho_engine::provider::discovery::discover_custom_provider_models(name, base_url, key.as_deref())
        .await
        .ok()
}

async fn refresh_single_custom_provider(
    store: &mut ModelStore,
    auth: &AuthStore,
    name: &str,
    spec: &rho_harness_core::config::ProviderConfig,
) {
    if let Some(discovered) = discover_custom(name, &spec.base_url, auth).await {
        let _ = store.set_models_async(name, discovered).await;
    }
}

async fn refresh_custom_provider_models(
    store: &mut ModelStore,
    auth: &AuthStore,
    custom: BTreeMap<String, rho_harness_core::config::ProviderConfig>,
) {
    for (name, spec) in &custom {
        refresh_single_custom_provider(store, auth, name, spec).await;
    }
}

async fn refresh_local_models(store: &mut ModelStore, auth: &AuthStore) {
    if let Ok(discovered) = discover_provider_models(ProviderId::Local, auth).await {
        let _ = store.set_models_async("local", discovered).await;
    }
}

async fn refresh_discovered_models(
    config_dir: PathBuf,
    auth: AuthStore,
    custom_providers: BTreeMap<String, rho_harness_core::config::ProviderConfig>,
) {
    let mut store = ModelStore::load_async(config_dir.join("models-store.json")).await;
    refresh_local_models(&mut store, &auth).await;
    refresh_auth_provider_models(&mut store, &auth).await;
    refresh_custom_provider_models(&mut store, &auth, custom_providers).await;
}

/// Spawns a background task to refresh models from live provider endpoints.
pub fn spawn_background_model_refresh(config: &Config, auth_store: &AuthStore) {
    let config_dir = config.config_dir.clone();
    let auth = auth_store.clone();
    let custom = config.providers.clone();
    tokio::spawn(refresh_discovered_models(config_dir, auth, custom));
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_engine::provider::discovery::DiscoveredModel;
    use rho_harness_core::config::ProviderConfig;
    use std::collections::HashMap;

    fn discovered(id: &str, provider: &str, description: &str) -> DiscoveredModel {
        DiscoveredModel::new(id, id, provider, description)
    }

    fn model_item(id: &str, provider: &str, description: &str) -> ModelItem {
        ModelItem {
            id: id.to_string(),
            provider: provider.to_string(),
            description: description.to_string(),
        }
    }

    fn store_with(entries: &[(&str, Vec<DiscoveredModel>)]) -> ModelStore {
        let models: HashMap<String, Vec<DiscoveredModel>> = entries
            .iter()
            .map(|(provider, models)| ((*provider).to_string(), models.clone()))
            .collect();
        serde_json::from_value(serde_json::json!({ "updated_at_ms": 0, "models": models })).unwrap()
    }

    fn auth_with(dir: &tempfile::TempDir, providers: &[&str]) -> AuthStore {
        let mut auth = AuthStore::load(dir.path().join("auth.json")).unwrap();
        for provider in providers {
            auth.set_key(provider, "test-key").unwrap();
        }
        auth
    }

    fn config_with_custom(active: &str, providers: &[(&str, &str)]) -> Config {
        Config {
            provider: active.to_string(),
            providers: providers
                .iter()
                .map(|(name, base_url)| {
                    (
                        (*name).to_string(),
                        ProviderConfig {
                            base_url: (*base_url).to_string(),
                            ..Default::default()
                        },
                    )
                })
                .collect(),
            ..Default::default()
        }
    }

    fn summarize(models: &[ModelItem]) -> Vec<(&str, &str, &str)> {
        models
            .iter()
            .map(|m| (m.id.as_str(), m.provider.as_str(), m.description.as_str()))
            .collect()
    }

    #[test]
    fn merge_provider_presets_keeps_cached_entries_and_adds_missing_presets() {
        let cached = discovered("gpt-4o", "chatgpt", "cached gpt-4o");
        let custom = discovered("acme-cache", "chatgpt", "cached custom");
        let merged = merge_provider_presets("chatgpt", vec![cached.clone(), custom.clone()]);
        let ids: Vec<&str> = merged.iter().map(|m| m.id.as_str()).collect();

        assert_eq!(merged.iter().filter(|m| m.id == "gpt-4o").count(), 1);
        assert!(merged.contains(&cached));
        assert!(merged.contains(&custom));
        assert!(ids.contains(&"gpt-6-luna"));
        assert_eq!(ids.first(), Some(&"gpt-6.1-sol"));
    }

    #[test]
    fn merge_provider_presets_adds_provider_specific_or_fallback_presets() {
        let cases = [
            ("chatgpt", "gpt-6-luna"),
            ("local", "llama3.2:latest"),
            ("acme", "acme-default"),
        ];
        for (provider, preset_id) in cases {
            let merged = merge_provider_presets(provider, Vec::new());
            assert!(
                merged.iter().any(|m| m.id == preset_id),
                "{provider} should include {preset_id}"
            );
        }
    }

    #[test]
    fn configured_provider_models_cover_auth_and_store_providers_but_not_local() {
        let dir = tempfile::tempdir().unwrap();
        let auth = auth_with(&dir, &["chatgpt"]);
        let store = store_with(&[
            ("acme", vec![discovered("acme-model", "acme", "cached acme")]),
            ("local", vec![discovered("llama-cached", "local", "local cache")]),
            ("ollama", vec![discovered("ollama-cached", "ollama", "ollama cache")]),
        ]);
        let mut models = Vec::new();
        append_configured_provider_models(&mut models, &store, &auth);

        assert!(models.iter().any(|m| m.id == "gpt-6-luna" && m.provider == "chatgpt"));
        assert!(models.contains(&model_item("acme-model", "acme", "cached acme")));
        assert!(models.contains(&model_item("acme-default", "acme", "custom model")));
        assert!(models.iter().all(|m| m.provider != "claude"));
        assert!(models.iter().all(|m| m.provider != "local" && m.provider != "ollama"));
        let chatgpt_ids: Vec<&str> = models
            .iter()
            .filter(|m| m.provider == "chatgpt")
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(chatgpt_ids.first(), Some(&"gpt-6.1-sol"));
    }

    #[test]
    fn configured_provider_models_skip_entries_already_listed() {
        let dir = tempfile::tempdir().unwrap();
        let auth = auth_with(&dir, &["chatgpt"]);
        let store = store_with(&[]);
        let mut models = vec![model_item("gpt-6-luna", "chatgpt", "already listed")];
        append_configured_provider_models(&mut models, &store, &auth);

        assert_eq!(models.iter().filter(|m| m.id == "gpt-6-luna").count(), 1);
        assert!(models.contains(&model_item("gpt-6-luna", "chatgpt", "already listed")));
    }

    #[test]
    fn custom_provider_models_use_cached_catalog_or_endpoint_placeholder() {
        let endpoint = "endpoint: https://acme.test/v1";
        let cached = vec![
            discovered("m-3.1", "acme", "desc 3.1"),
            discovered("m-3.8", "acme", "desc 3.8"),
        ];
        let cached_expected = vec![("m-3.8", "acme", "desc 3.8"), ("m-3.1", "acme", "desc 3.1")];
        type Expected = Vec<(&'static str, &'static str, &'static str)>;
        let cases: Vec<(&str, Option<Vec<DiscoveredModel>>, Expected)> = vec![
            ("other", None, vec![("acme-default", "acme", endpoint)]),
            ("acme", None, vec![]),
            ("other", Some(cached.clone()), cached_expected.clone()),
            ("acme", Some(cached), cached_expected),
        ];
        for (active, cached, expected) in cases {
            let store = match cached {
                Some(models) => store_with(&[("acme", models)]),
                None => store_with(&[]),
            };
            let config = config_with_custom(active, &[("acme", "https://acme.test/v1")]);
            let mut models = Vec::new();
            append_custom_provider_models(&mut models, &config, &store);
            assert_eq!(summarize(&models), expected, "active provider {active}");
        }
    }

    #[test]
    fn custom_provider_models_keep_existing_entries_unique() {
        let store = store_with(&[("acme", vec![discovered("m-3.8", "acme", "desc 3.8")])]);
        let config = config_with_custom("other", &[("acme", "https://acme.test/v1")]);
        let mut models = vec![model_item("m-3.8", "acme", "already listed")];
        append_custom_provider_models(&mut models, &config, &store);

        assert_eq!(summarize(&models), vec![("m-3.8", "acme", "already listed")]);
    }
}
