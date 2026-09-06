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
    crate::install_crypto_provider();
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default()
});

static OLLAMA_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    crate::install_crypto_provider();
    reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .no_proxy()
        .build()
        .unwrap_or_default()
});

fn map_openai_models(data: Vec<OpenAiModelItem>, provider_name: &str) -> Vec<DiscoveredModel> {
    let mut models = Vec::new();
    for item in data {
        models.push(DiscoveredModel {
            context_tokens: None,
            id: item.id.clone(),
            name: item.id.clone(),
            provider: provider_name.to_string(),
            description: format_context_desc(&item.id),
        });
    }
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models
}

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
        let models = map_openai_models(body.data, provider_name);
        if !models.is_empty() {
            return Ok(models);
        }
    }

    Ok(default_presets_for(provider_name))
}

pub(crate) async fn discover_ollama_models() -> Result<Vec<DiscoveredModel>> {
    let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let spec = OllamaCatalog {
        client: &OLLAMA_CLIENT,
        host: &host,
        auth: None,
        provider: "local",
        fallback_description: "local model",
    };
    discover_ollama_catalog(spec).await
}

/// ollama.com is a remote Ollama host, so the native `/api/tags` + `/api/show`
/// endpoints report the cloud catalog and each model's real context length.
pub(crate) async fn discover_ollama_cloud_models(api_key: &str) -> Result<Vec<DiscoveredModel>> {
    let spec = OllamaCatalog {
        client: &HTTP_CLIENT,
        host: crate::ollama::CLOUD_HOST,
        auth: Some(api_key),
        provider: "ollama-cloud",
        fallback_description: "cloud model",
    };
    let models = discover_ollama_catalog(spec).await?;
    if models.is_empty() {
        return Ok(super::presets::ollama_cloud_preset_models());
    }
    Ok(models)
}

struct OllamaCatalog<'a> {
    client: &'a reqwest::Client,
    host: &'a str,
    auth: Option<&'a str>,
    provider: &'a str,
    fallback_description: &'a str,
}

async fn convert_ollama_models(
    models: Vec<OllamaTagItem>,
    spec: &OllamaCatalog<'_>,
    host: &str,
) -> Vec<DiscoveredModel> {
    let mut out = Vec::new();
    for item in models {
        let id = item.name;
        let context_tokens = ollama_context_length(spec.client, host, &id).await;
        let description = context_tokens
            .map(format_context_tokens)
            .unwrap_or_else(|| spec.fallback_description.to_string());
        out.push(DiscoveredModel {
            name: id.clone(),
            id,
            provider: spec.provider.to_string(),
            description,
            context_tokens,
        });
    }
    out
}

fn build_tags_request(client: &reqwest::Client, endpoint: &str, auth: Option<&str>) -> reqwest::RequestBuilder {
    let mut req = client.get(endpoint);
    if let Some(key) = auth {
        req = req.header("Authorization", format!("Bearer {}", key.trim()));
    }
    req
}

async fn fetch_ollama_tags(spec: &OllamaCatalog<'_>, endpoint: &str) -> Option<Vec<OllamaTagItem>> {
    let resp = build_tags_request(spec.client, endpoint, spec.auth).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body = resp.json::<OllamaTagsResponse>().await.ok()?;
    Some(body.models)
}

async fn discover_ollama_catalog(spec: OllamaCatalog<'_>) -> Result<Vec<DiscoveredModel>> {
    let host = spec.host.trim_end_matches('/');
    let endpoint = format!("{host}/api/tags");

    if let Some(models) = fetch_ollama_tags(&spec, &endpoint).await {
        let converted = convert_ollama_models(models, &spec, host).await;
        if !converted.is_empty() {
            return Ok(converted);
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

fn map_anthropic_models(data: Vec<AnthropicModelItem>) -> Vec<DiscoveredModel> {
    data.into_iter()
        .map(|item| DiscoveredModel {
            context_tokens: None,
            id: item.id.clone(),
            name: item.display_name.unwrap_or_else(|| item.id.clone()),
            provider: "anthropic".to_string(),
            description: format_context_desc(&item.id),
        })
        .collect()
}

pub(crate) async fn discover_anthropic_models(api_key: &str) -> Result<Vec<DiscoveredModel>> {
    let req = HTTP_CLIENT
        .get("https://api.anthropic.com/v1/models")
        .header("x-api-key", api_key.trim())
        .header("anthropic-version", "2023-06-01");
    if let Ok(resp) = req.send().await
        && resp.status().is_success()
        && let Ok(body) = resp.json::<AnthropicModelsResponse>().await
    {
        let models = map_anthropic_models(body.data);
        if !models.is_empty() {
            return Ok(models);
        }
    }

    Ok(anthropic_preset_models())
}

fn map_gemini_models(models: Vec<GeminiModelItem>) -> Vec<DiscoveredModel> {
    models
        .into_iter()
        .filter_map(|item| {
            let id = item.name.strip_prefix("models/").unwrap_or(&item.name);
            if id.starts_with("gemini") {
                Some(DiscoveredModel {
                    context_tokens: None,
                    id: id.to_string(),
                    name: item.display_name.unwrap_or_else(|| id.to_string()),
                    provider: "gemini".to_string(),
                    description: format_context_desc(id),
                })
            } else {
                None
            }
        })
        .collect()
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
        let models = map_gemini_models(body.models);
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
