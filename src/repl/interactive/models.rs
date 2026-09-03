//! Dynamic model discovery and capability descriptors for autocomplete and model switcher.

use super::completion::ModelItem;
use rho_engine::auth::AuthStore;
use rho_engine::provider::discovery::discover_provider_models;
use rho_engine::provider::store::ModelStore;
use rho_harness_core::config::Config;
use rho_harness_core::provider::ProviderId;
use std::str::FromStr;

/// Dynamically discovers models available to the current user from active configuration,
/// local Ollama, live/cached provider discovery catalogs, and custom endpoints.
pub fn discover_models(config: &Config, auth_store: &AuthStore) -> Vec<ModelItem> {
    let mut models = Vec::new();
    let model_store = ModelStore::load(config.config_dir.join("models-store.json"));

    // 1. Current active model always listed first with active status
    let active_ctx = model_store
        .get_models(&config.provider)
        .and_then(|models| models.iter().find(|m| m.id == config.model))
        .and_then(|m| m.context_tokens)
        .unwrap_or_else(|| rho_harness_core::tokens::context_window_size(&config.model));
    let active_ctx_str = if active_ctx >= 1_000_000 {
        format!("{}M ctx", active_ctx / 1_000_000)
    } else {
        format!("{}k ctx", active_ctx / 1000)
    };
    models.push(ModelItem {
        id: config.model.clone(),
        provider: config.provider.clone(),
        description: format!("{active_ctx_str} · active"),
    });

    // 2. Local models (always available locally without API keys)
    if let Some(cached_local) = model_store
        .get_models("local")
        .or_else(|| model_store.get_models("ollama"))
    {
        for m in cached_local {
            if !models
                .iter()
                .any(|existing| existing.id == m.id && existing.provider == m.provider)
            {
                models.push(ModelItem {
                    id: m.id.clone(),
                    provider: m.provider.clone(),
                    description: m.description.clone(),
                });
            }
        }
    }

    // 3. Models from configured providers in auth_store & model_store. The
    // store keys are merged in too so a session started before a fresh login
    // still sees the newly authenticated provider's catalog.
    let mut configured_providers: Vec<String> = auth_store.list_configured_providers();
    let store_providers: Vec<String> = model_store
        .providers()
        .filter(|prov| !configured_providers.contains(prov))
        .cloned()
        .collect();
    configured_providers.extend(store_providers);
    for prov in &configured_providers {
        if prov == "local" || prov == "ollama" {
            continue; // Already handled above
        }
        if let Some(cached) = model_store.get_models(prov) {
            for m in cached {
                if !models
                    .iter()
                    .any(|existing| existing.id == m.id && existing.provider == m.provider)
                {
                    models.push(ModelItem {
                        id: m.id.clone(),
                        provider: m.provider.clone(),
                        description: m.description.clone(),
                    });
                }
            }
        }
    }

    // 4. Custom configured providers from config.toml ([providers.<name>])
    for (name, spec) in &config.providers {
        if let Some(cached) = model_store.get_models(name) {
            for m in cached {
                if !models
                    .iter()
                    .any(|existing| existing.id == m.id && existing.provider == m.provider)
                {
                    models.push(ModelItem {
                        id: m.id.clone(),
                        provider: m.provider.clone(),
                        description: m.description.clone(),
                    });
                }
            }
        } else if name != &config.provider {
            models.push(ModelItem {
                id: format!("{name}-default"),
                provider: name.clone(),
                description: format!("endpoint: {}", spec.base_url),
            });
        }
    }

    models
}

/// Spawns a background task to refresh models from live provider endpoints.
pub fn spawn_background_model_refresh(config: &Config, auth_store: &AuthStore) {
    let config_dir = config.config_dir.clone();
    let auth_store_clone = auth_store.clone();
    let configured_providers = auth_store.list_configured_providers();
    let custom_providers = config.providers.clone();

    tokio::spawn(async move {
        let mut store = ModelStore::load(config_dir.join("models-store.json"));

        // Always discover local models
        if let Ok(discovered) = discover_provider_models(ProviderId::Local, &auth_store_clone).await {
            let _ = store.set_models("local", discovered);
        }

        // Discover configured authenticated providers
        for prov_str in configured_providers {
            if prov_str == "local" || prov_str == "ollama" {
                continue;
            }
            if let Ok(id) = ProviderId::from_str(&prov_str)
                && let Ok(discovered) = discover_provider_models(id, &auth_store_clone).await
            {
                let _ = store.set_models(&prov_str, discovered);
            }
        }

        // Discover custom endpoints
        for (name, spec) in custom_providers {
            let key = auth_store_clone.get_key_sync(&name).ok().flatten();
            if let Ok(discovered) =
                rho_engine::provider::discovery::discover_custom_provider_models(&name, &spec.base_url, key.as_deref())
                    .await
            {
                let _ = store.set_models(&name, discovered);
            }
        }
    });
}
