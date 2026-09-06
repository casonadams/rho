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
    let ctx_str = if active_ctx >= 1_000_000 {
        format!("{}M ctx", active_ctx / 1_000_000)
    } else {
        format!("{}k ctx", active_ctx / 1000)
    };
    models.push(ModelItem {
        id: config.model.clone(),
        provider: config.provider.clone(),
        description: format!("{ctx_str} · active"),
    });
}

fn append_local_models(models: &mut Vec<ModelItem>, store: &ModelStore) {
    let local = store
        .get_models("local")
        .or_else(|| store.get_models("ollama"))
        .cloned()
        .unwrap_or_else(|| rho_engine::provider::discovery::default_presets_for("local"));
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

fn append_configured_provider_models(models: &mut Vec<ModelItem>, (store, auth_store): (&ModelStore, &AuthStore)) {
    let mut configured: Vec<String> = auth_store.list_configured_providers();
    for prov in store.providers() {
        if !configured.contains(prov) {
            configured.push(prov.clone());
        }
    }
    for prov in configured.iter().filter(|p| *p != "local" && *p != "ollama") {
        let prov_models = store
            .get_models(prov)
            .cloned()
            .unwrap_or_else(|| rho_engine::provider::discovery::default_presets_for(prov));
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
    append_configured_provider_models(&mut models, (&model_store, auth_store));
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
    (name, spec): (&str, &rho_harness_core::config::ProviderConfig),
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
        refresh_single_custom_provider(store, auth, (name, spec)).await;
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
