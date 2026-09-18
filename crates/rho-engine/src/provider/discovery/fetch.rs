//! Dynamic network model discovery for authenticated endpoints.

use super::DiscoveredModel;
use super::antigravity::collapse_antigravity_catalog;
use super::presets::{
    anthropic_preset_models, antigravity_preset_models, default_presets_for, format_context_desc,
    format_context_tokens, gemini_preset_models,
};
use crate::provider::builders::SHARED_HTTP_CLIENT;
use rho_harness_core::error::Result;
use rig::client::ModelListingClient;

fn map_rig_models(models: Vec<rig::model::Model>, provider_name: &str) -> Vec<DiscoveredModel> {
    let mut out: Vec<DiscoveredModel> = models
        .into_iter()
        .filter_map(|item| {
            let id = if provider_name == "gemini" {
                let trimmed = item.id.strip_prefix("models/").unwrap_or(&item.id);
                if !trimmed.starts_with("gemini") {
                    return None;
                }
                trimmed.to_string()
            } else {
                item.id
            };
            if id.trim().is_empty() {
                return None;
            }
            let description = item
                .context_length
                .map(|ctx| format_context_tokens(ctx as usize))
                .unwrap_or_else(|| format_context_desc(&id));
            Some(DiscoveredModel {
                context_tokens: item.context_length.map(|l| l as usize),
                name: item.name.unwrap_or_else(|| id.clone()),
                id,
                provider: provider_name.to_string(),
                description,
            })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

pub(crate) async fn discover_openai_compatible(
    provider_name: &str,
    base_url: &str,
    api_key: &str,
) -> Result<Vec<DiscoveredModel>> {
    let key = if api_key.trim().is_empty() { "" } else { api_key.trim() };
    if let Ok(client) = rig::providers::openai::Client::builder()
        .http_client(SHARED_HTTP_CLIENT.clone())
        .base_url(base_url)
        .api_key(key)
        .build()
        && let Ok(list) = client.list_models().await
    {
        let models = map_rig_models(list.data, provider_name);
        if !models.is_empty() {
            return Ok(models);
        }
    }

    Ok(default_presets_for(provider_name))
}

pub(crate) async fn discover_ollama_models() -> Result<Vec<DiscoveredModel>> {
    let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let builder = rig::providers::ollama::Client::builder()
        .http_client(SHARED_HTTP_CLIENT.clone())
        .base_url(&host)
        .api_key("");

    if let Ok(client) = builder.build()
        && let Ok(list) = client.list_models().await
    {
        let models = map_rig_models(list.data, "local");
        if !models.is_empty() {
            return Ok(models);
        }
    }

    Ok(Vec::new())
}

pub(crate) async fn discover_ollama_cloud_models(api_key: &str) -> Result<Vec<DiscoveredModel>> {
    let models = discover_openai_compatible("ollama-cloud", "https://ollama.com/v1", api_key).await?;
    if models.is_empty() {
        return Ok(super::presets::ollama_cloud_preset_models());
    }
    Ok(models)
}

pub fn ollama_context_from_info(model_info: &serde_json::Map<String, serde_json::Value>) -> Option<usize> {
    model_info
        .iter()
        .find(|(key, _)| key.ends_with(".context_length"))
        .and_then(|(_, value)| value.as_u64().map(|n| n as usize))
}

pub(crate) async fn discover_anthropic_models(api_key: &str) -> Result<Vec<DiscoveredModel>> {
    if !api_key.trim().is_empty() {
        let builder = rig::providers::anthropic::Client::builder()
            .http_client(SHARED_HTTP_CLIENT.clone())
            .api_key(api_key.trim());
        if let Ok(client) = builder.build()
            && let Ok(list) = client.list_models().await
        {
            let models = map_rig_models(list.data, "anthropic");
            if !models.is_empty() {
                return Ok(models);
            }
        }
    }

    Ok(anthropic_preset_models())
}

pub(crate) async fn discover_gemini_models(api_key: &str) -> Result<Vec<DiscoveredModel>> {
    if !api_key.trim().is_empty() {
        let builder = rig::providers::gemini::Client::builder()
            .http_client(SHARED_HTTP_CLIENT.clone())
            .api_key(api_key.trim());
        if let Ok(client) = builder.build()
            && let Ok(list) = client.list_models().await
        {
            let models = map_rig_models(list.data, "gemini");
            if !models.is_empty() {
                return Ok(models);
            }
        }
    }

    Ok(gemini_preset_models())
}

pub(crate) async fn discover_antigravity_models(token: &str, project_id: &str) -> Result<Vec<DiscoveredModel>> {
    if let Some(ids) = crate::antigravity::discover_models(token, project_id).await {
        return Ok(collapse_antigravity_catalog(ids));
    }
    Ok(antigravity_preset_models())
}
