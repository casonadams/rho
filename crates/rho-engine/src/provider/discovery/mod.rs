//! Live dynamic model discovery from authenticated provider endpoints.

pub mod antigravity;
pub(crate) mod fetch;
pub mod presets;

#[cfg(test)]
mod tests;

pub use antigravity::sort_models_newest_first;
pub use fetch::ollama_context_from_info;
pub use presets::{
    anthropic_preset_models, antigravity_preset_models, chatgpt_codex_models, claude_preset_models,
    cohere_preset_models, copilot_models, deepseek_preset_models, default_presets_for, format_context_tokens,
    gemini_preset_models, groq_preset_models, mistral_preset_models, ollama_cloud_preset_models, openai_preset_models,
    openrouter_preset_models, xai_preset_models,
};

use crate::auth::AuthStore;
use rho_harness_core::error::Result;
use rho_harness_core::provider::ProviderId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredModel {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub description: String,
    #[serde(default)]
    pub context_tokens: Option<usize>,
}

async fn discover_keyed_openai_compatible(
    (name, url): (&str, &str),
    auth_store: &AuthStore,
) -> Result<Vec<DiscoveredModel>> {
    if let Some(key) = auth_store.get_key_sync(name)? {
        fetch::discover_openai_compatible(name, url, &key).await
    } else {
        Ok(default_presets_for(name))
    }
}

async fn discover_antigravity(auth_store: &AuthStore) -> Result<Vec<DiscoveredModel>> {
    if let Some(key) = auth_store.get_key_sync("antigravity")? {
        let project_id = match auth_store.get_credential("antigravity") {
            Some(rho_harness_core::auth::StoredCredential::OAuth {
                account_id: Some(id), ..
            }) => id.clone(),
            _ => crate::auth::antigravity::stable_project_id("antigravity-default"),
        };
        fetch::discover_antigravity_models(&key, &project_id).await
    } else {
        Ok(antigravity_preset_models())
    }
}

async fn discover_anthropic(auth_store: &AuthStore) -> Result<Vec<DiscoveredModel>> {
    if let Some(key) = auth_store.get_key_sync("anthropic")? {
        fetch::discover_anthropic_models(&key).await
    } else {
        Ok(anthropic_preset_models())
    }
}

async fn discover_gemini(auth_store: &AuthStore) -> Result<Vec<DiscoveredModel>> {
    if let Some(key) = auth_store.get_key_sync("gemini")? {
        fetch::discover_gemini_models(&key).await
    } else {
        Ok(gemini_preset_models())
    }
}

async fn discover_ollama_cloud(auth_store: &AuthStore) -> Result<Vec<DiscoveredModel>> {
    if let Some(key) = auth_store.get_key_sync("ollama-cloud")? {
        fetch::discover_ollama_cloud_models(&key).await
    } else {
        Ok(ollama_cloud_preset_models())
    }
}

type DiscoveryFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<DiscoveredModel>>> + Send + 'a>>;

fn keyed_discovery<'a>(
    (name, base_url): (&'static str, &'static str),
    auth_store: &'a AuthStore,
) -> DiscoveryFuture<'a> {
    Box::pin(discover_keyed_openai_compatible((name, base_url), auth_store))
}

fn dispatch_provider_discovery<'a>(provider: ProviderId, auth_store: &'a AuthStore) -> DiscoveryFuture<'a> {
    match provider {
        ProviderId::ChatGpt => Box::pin(async { Ok(chatgpt_codex_models()) }),
        ProviderId::ClaudeCode => Box::pin(async { Ok(claude_preset_models()) }),
        ProviderId::Copilot => Box::pin(async { Ok(copilot_models()) }),
        ProviderId::Local => Box::pin(fetch::discover_ollama_models()),
        ProviderId::OpenAi => keyed_discovery(("openai", "https://api.openai.com/v1"), auth_store),
        ProviderId::OpenRouter => keyed_discovery(("openrouter", "https://openrouter.ai/api/v1"), auth_store),
        ProviderId::Groq => keyed_discovery(("groq", "https://api.groq.com/openai/v1"), auth_store),
        ProviderId::DeepSeek => keyed_discovery(("deepseek", "https://api.deepseek.com"), auth_store),
        ProviderId::Anthropic => Box::pin(discover_anthropic(auth_store)),
        ProviderId::Gemini => Box::pin(discover_gemini(auth_store)),
        ProviderId::Antigravity => Box::pin(discover_antigravity(auth_store)),
        ProviderId::OllamaCloud => Box::pin(discover_ollama_cloud(auth_store)),
        _ => Box::pin(async move { Ok(default_presets_for(&provider.to_string())) }),
    }
}

pub async fn discover_provider_models(provider: ProviderId, auth_store: &AuthStore) -> Result<Vec<DiscoveredModel>> {
    dispatch_provider_discovery(provider, auth_store).await
}

pub async fn discover_custom_provider_models(
    name: &str,
    base_url: &str,
    api_key: Option<&str>,
) -> Result<Vec<DiscoveredModel>> {
    let key = api_key.unwrap_or_default();
    fetch::discover_openai_compatible(name, base_url, key).await
}
