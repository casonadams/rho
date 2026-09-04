//! Live dynamic model discovery from authenticated provider endpoints.

pub mod antigravity;
pub(crate) mod fetch;
pub mod presets;

#[cfg(test)]
mod tests;

pub use antigravity::sort_models_newest_first;
pub use fetch::ollama_context_from_info;
pub use presets::{
    anthropic_preset_models, antigravity_preset_models, chatgpt_codex_models, cohere_preset_models, copilot_models,
    deepseek_preset_models, format_context_tokens, gemini_preset_models, groq_preset_models, mistral_preset_models,
    ollama_cloud_preset_models, openai_preset_models, openrouter_preset_models, xai_preset_models,
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

pub async fn discover_provider_models(provider: ProviderId, auth_store: &AuthStore) -> Result<Vec<DiscoveredModel>> {
    match provider {
        ProviderId::ChatGpt => Ok(chatgpt_codex_models()),
        ProviderId::Copilot => Ok(copilot_models()),
        ProviderId::Local => fetch::discover_ollama_models().await,
        ProviderId::OpenAi => {
            if let Some(key) = auth_store.get_key_sync("openai")? {
                fetch::discover_openai_compatible("openai", "https://api.openai.com/v1", &key).await
            } else {
                Ok(openai_preset_models())
            }
        }
        ProviderId::OpenRouter => {
            if let Some(key) = auth_store.get_key_sync("openrouter")? {
                fetch::discover_openai_compatible("openrouter", "https://openrouter.ai/api/v1", &key).await
            } else {
                Ok(openrouter_preset_models())
            }
        }
        ProviderId::Groq => {
            if let Some(key) = auth_store.get_key_sync("groq")? {
                fetch::discover_openai_compatible("groq", "https://api.groq.com/openai/v1", &key).await
            } else {
                Ok(groq_preset_models())
            }
        }
        ProviderId::DeepSeek => {
            if let Some(key) = auth_store.get_key_sync("deepseek")? {
                fetch::discover_openai_compatible("deepseek", "https://api.deepseek.com", &key).await
            } else {
                Ok(deepseek_preset_models())
            }
        }
        ProviderId::Anthropic => {
            if let Some(key) = auth_store.get_key_sync("anthropic")? {
                fetch::discover_anthropic_models(&key).await
            } else {
                Ok(anthropic_preset_models())
            }
        }
        ProviderId::Gemini => {
            if let Some(key) = auth_store.get_key_sync("gemini")? {
                fetch::discover_gemini_models(&key).await
            } else {
                Ok(gemini_preset_models())
            }
        }
        ProviderId::Antigravity => {
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
        ProviderId::Mistral => Ok(mistral_preset_models()),
        ProviderId::XAi => Ok(xai_preset_models()),
        ProviderId::Cohere => Ok(cohere_preset_models()),
        ProviderId::OllamaCloud => {
            if let Some(key) = auth_store.get_key_sync("ollama-cloud")? {
                fetch::discover_openai_compatible("ollama-cloud", "https://ollama.com/v1", &key).await
            } else {
                Ok(ollama_cloud_preset_models())
            }
        }
    }
}

pub async fn discover_custom_provider_models(
    name: &str,
    base_url: &str,
    api_key: Option<&str>,
) -> Result<Vec<DiscoveredModel>> {
    let key = api_key.unwrap_or_default();
    fetch::discover_openai_compatible(name, base_url, key).await
}
