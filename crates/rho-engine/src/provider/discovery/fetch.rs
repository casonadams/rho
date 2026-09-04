//! Dynamic network model discovery for authenticated endpoints.

use super::DiscoveredModel;
use super::antigravity::collapse_antigravity_catalog;
use super::presets::{
    anthropic_preset_models, antigravity_preset_models, default_presets_for, format_context_desc,
    format_context_tokens, gemini_preset_models,
};
use rho_harness_core::error::Result;
use serde::Deserialize;
use std::sync::LazyLock;
use std::time::Duration;

static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default()
});

static OLLAMA_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .no_proxy()
        .build()
        .unwrap_or_default()
});

pub(crate) async fn discover_openai_compatible(
    provider_name: &str,
    base_url: &str,
    api_key: &str,
) -> Result<Vec<DiscoveredModel>> {
    let endpoint = format!("{}/models", base_url.trim_end_matches('/'));
    let mut req = HTTP_CLIENT.get(&endpoint);
    if !api_key.trim().is_empty() {
        req = req.header("Authorization", format!("Bearer {}", api_key.trim()));
    }

    if let Ok(resp) = req.send().await
        && resp.status().is_success()
        && let Ok(body) = resp.json::<OpenAiModelsResponse>().await
    {
        let mut models = Vec::new();
        for item in body.data {
            let desc = format_context_desc(&item.id);
            models.push(DiscoveredModel {
                context_tokens: None,
                id: item.id.clone(),
                name: item.id.clone(),
                provider: provider_name.to_string(),
                description: desc,
            });
        }
        if !models.is_empty() {
            models.sort_by(|a, b| a.id.cmp(&b.id));
            return Ok(models);
        }
    }

    Ok(default_presets_for(provider_name))
}

pub(crate) async fn discover_ollama_models() -> Result<Vec<DiscoveredModel>> {
    let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let endpoint = format!("{}/api/tags", host.trim_end_matches('/'));

    if let Ok(resp) = OLLAMA_CLIENT.get(&endpoint).send().await
        && resp.status().is_success()
        && let Ok(body) = resp.json::<OllamaTagsResponse>().await
    {
        let mut models = Vec::new();
        for item in body.models {
            let id = item.name;
            let context_tokens = ollama_context_length(&OLLAMA_CLIENT, host.trim_end_matches('/'), &id).await;
            let description = context_tokens
                .map(format_context_tokens)
                .unwrap_or_else(|| "local model".to_string());
            models.push(DiscoveredModel {
                name: id.clone(),
                id,
                provider: "local".to_string(),
                description,
                context_tokens,
            });
        }
        if !models.is_empty() {
            return Ok(models);
        }
    }

    Ok(Vec::new())
}

async fn ollama_context_length(client: &reqwest::Client, host: &str, model: &str) -> Option<usize> {
    let endpoint = format!("{}/api/show", host);
    let resp = client
        .post(&endpoint)
        .json(&serde_json::json!({ "model": model }))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: OllamaShowResponse = resp.json().await.ok()?;
    ollama_context_from_info(&body.model_info)
}

/// Ollama reports the architecture's context window in `model_info` under a
/// key named `\u{3carch}\u{3e}.context_length` (e.g. `qwen3_5.context_length`).
pub fn ollama_context_from_info(model_info: &serde_json::Map<String, serde_json::Value>) -> Option<usize> {
    model_info
        .iter()
        .find(|(key, _)| key.ends_with(".context_length"))
        .and_then(|(_, value)| value.as_u64().map(|n| n as usize))
}

pub(crate) async fn discover_anthropic_models(api_key: &str) -> Result<Vec<DiscoveredModel>> {
    if let Ok(resp) = HTTP_CLIENT
        .get("https://api.anthropic.com/v1/models")
        .header("x-api-key", api_key.trim())
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
        && resp.status().is_success()
        && let Ok(body) = resp.json::<AnthropicModelsResponse>().await
    {
        let mut models = Vec::new();
        for item in body.data {
            let desc = format_context_desc(&item.id);
            models.push(DiscoveredModel {
                context_tokens: None,
                id: item.id.clone(),
                name: item.display_name.unwrap_or_else(|| item.id.clone()),
                provider: "anthropic".to_string(),
                description: desc,
            });
        }
        if !models.is_empty() {
            return Ok(models);
        }
    }

    Ok(anthropic_preset_models())
}

pub(crate) async fn discover_gemini_models(api_key: &str) -> Result<Vec<DiscoveredModel>> {
    let endpoint = format!(
        "https://generativelanguage.googleapis.com/v1beta/models?key={}",
        api_key.trim()
    );
    if let Ok(resp) = HTTP_CLIENT.get(&endpoint).send().await
        && resp.status().is_success()
        && let Ok(body) = resp.json::<GeminiModelsResponse>().await
    {
        let mut models = Vec::new();
        for item in body.models {
            let id = item.name.strip_prefix("models/").unwrap_or(&item.name);
            if id.starts_with("gemini") {
                models.push(DiscoveredModel {
                    context_tokens: None,
                    id: id.to_string(),
                    name: item.display_name.unwrap_or_else(|| id.to_string()),
                    provider: "gemini".to_string(),
                    description: format_context_desc(id),
                });
            }
        }
        if !models.is_empty() {
            return Ok(models);
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

#[derive(Deserialize)]
struct OpenAiModelsResponse {
    data: Vec<OpenAiModelItem>,
}

#[derive(Deserialize)]
struct OpenAiModelItem {
    id: String,
}

#[derive(Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaTagItem>,
}

#[derive(Deserialize)]
struct OllamaTagItem {
    name: String,
}

#[derive(Deserialize)]
struct OllamaShowResponse {
    #[serde(default)]
    model_info: serde_json::Map<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct AnthropicModelsResponse {
    data: Vec<AnthropicModelItem>,
}

#[derive(Deserialize)]
struct AnthropicModelItem {
    id: String,
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct GeminiModelsResponse {
    models: Vec<GeminiModelItem>,
}

#[derive(Deserialize)]
struct GeminiModelItem {
    name: String,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}
