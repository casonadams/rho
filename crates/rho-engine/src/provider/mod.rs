pub mod discovery;
pub mod store;

pub use store::ModelStore;

use crate::auth::AuthStore;
use rho_harness_core::config::Config;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::provider::ProviderId;
use rig::agent::ModelHandle;
use rig::client::CompletionClient;
use std::str::FromStr;

#[cfg(test)]
mod tests;

pub struct ProviderFactory;

impl ProviderFactory {
    pub fn create_model(config: &Config, model: &str, auth_store: &AuthStore) -> Result<ModelHandle> {
        let name = config.provider.trim();
        if let Ok(provider_id) = ProviderId::from_str(name) {
            return Self::create_model_for(provider_id, model, auth_store);
        }
        Self::create_custom_model(config, model, auth_store)
    }

    fn create_custom_model(config: &Config, model: &str, auth_store: &AuthStore) -> Result<ModelHandle> {
        let name = config.provider.trim();
        let spec = config.providers.get(&name.to_ascii_lowercase()).ok_or_else(|| {
            AppError::Provider(format!(
                "Unknown provider '{name}'. Configure it in config.toml as\n\
                     [providers.{name}]\nbase_url = \"https://...\"\n\
                     before selecting it."
            ))
        })?;

        rho_harness_core::net::validate_url(&spec.base_url, config.allow_private_network).map_err(|e| match e {
            AppError::Tool(message) => AppError::Provider(format!("Provider '{name}': {message}")),
            other => other,
        })?;

        let key = Self::custom_key(name, spec, auth_store)?.ok_or_else(|| {
            AppError::Auth(format!(
                "Missing API key for provider '{name}'. Set {} or run 'rho login {}'.",
                spec.key_env.as_deref().unwrap_or("its API key env var"),
                name
            ))
        })?;

        let client = rig::providers::openai::Client::builder()
            .api_key(key)
            .base_url(&spec.base_url)
            .build()
            .map_err(|e| AppError::Provider(format!("Failed to initialize provider '{name}': {e}")))?;
        Ok(ModelHandle::named(name, client.completion_model(model)))
    }

    fn custom_key(
        name: &str,
        spec: &rho_harness_core::config::ProviderConfig,
        auth_store: &AuthStore,
    ) -> Result<Option<String>> {
        if let Some(env_name) = spec.key_env.as_deref()
            && let Ok(value) = std::env::var(env_name)
        {
            let value = value.trim().to_string();
            if !value.is_empty() {
                return Ok(Some(value));
            }
        }
        auth_store.get_key_sync(name)
    }

    pub fn create_model_for(provider: ProviderId, model: &str, auth_store: &AuthStore) -> Result<ModelHandle> {
        if provider == ProviderId::Local {
            let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string());
            let client = rig::providers::ollama::Client::builder()
                .api_key("")
                .base_url(&host)
                .build()
                .map_err(|e| AppError::Provider(format!("Failed to initialize Ollama client: {e}")))?;
            return Ok(ModelHandle::named(provider.as_str(), client.completion_model(model)));
        }

        let key = auth_store.get_key_sync(provider.as_str())?.ok_or_else(|| {
            AppError::Auth(format!(
                "Missing API key for provider '{}'. Run 'rho login {}' or set {}.",
                provider.as_str(),
                provider.as_str(),
                provider.api_key_env().unwrap_or("API key")
            ))
        })?;

        let handle = match provider {
            ProviderId::Anthropic => {
                let client = rig::providers::anthropic::Client::new(key)
                    .map_err(|e| AppError::Provider(format!("Failed to initialize Anthropic client: {e}")))?;
                ModelHandle::named(provider.as_str(), client.completion_model(model))
            }
            ProviderId::OpenAi => {
                let client = rig::providers::openai::Client::new(key)
                    .map_err(|e| AppError::Provider(format!("Failed to initialize OpenAI client: {e}")))?;
                ModelHandle::named(provider.as_str(), client.completion_model(model))
            }
            ProviderId::ChatGpt => {
                let account_id = match auth_store.get_credential("chatgpt") {
                    Some(rho_harness_core::auth::StoredCredential::OAuth {
                        account_id: Some(id), ..
                    }) => Some(id.clone()),
                    _ => crate::auth::oauth::extract_chatgpt_account_id(&key),
                };

                let mut default_headers = reqwest::header::HeaderMap::new();
                default_headers.insert(
                    "OpenAI-Beta",
                    reqwest::header::HeaderValue::from_static("responses=experimental"),
                );
                default_headers.insert("originator", reqwest::header::HeaderValue::from_static("codex"));
                default_headers.insert("User-Agent", reqwest::header::HeaderValue::from_static("Codex/0.22.4"));
                if let Some(ref acc_id) = account_id
                    && let Ok(val) = reqwest::header::HeaderValue::from_str(acc_id)
                {
                    default_headers.insert("chatgpt-account-id", val.clone());
                    default_headers.insert("ChatGPT-Account-Id", val);
                }

                let http_client = reqwest::Client::builder()
                    .no_proxy()
                    .default_headers(default_headers)
                    .build()
                    .map_err(|e| AppError::Other(e.into()))?;

                let client = rig::providers::chatgpt::Client::builder()
                    .http_client(http_client)
                    .api_key(rig::providers::chatgpt::ChatGPTAuth::AccessToken {
                        access_token: key,
                        account_id,
                    })
                    .originator("codex")
                    .build()
                    .map_err(|e| AppError::Provider(format!("Failed to initialize ChatGPT Codex client: {e}")))?;
                ModelHandle::named(provider.as_str(), client.completion_model(model))
            }
            ProviderId::Copilot => {
                let client = rig::providers::copilot::Client::builder()
                    .github_access_token(key)
                    .build()
                    .map_err(|e| AppError::Provider(format!("Failed to initialize Copilot client: {e}")))?;
                ModelHandle::named(provider.as_str(), client.completion_model(model))
            }
            ProviderId::Gemini | ProviderId::Antigravity => {
                let http_client = reqwest::Client::builder()
                    .no_proxy()
                    .build()
                    .map_err(|e| AppError::Other(e.into()))?;

                let client = rig::providers::gemini::Client::builder()
                    .http_client(http_client)
                    .api_key(&key)
                    .build()
                    .map_err(|e| AppError::Provider(format!("Failed to initialize Gemini client: {e}")))?;
                ModelHandle::named(provider.as_str(), client.completion_model(model))
            }
            ProviderId::DeepSeek => {
                let client = rig::providers::deepseek::Client::new(key)
                    .map_err(|e| AppError::Provider(format!("Failed to initialize DeepSeek client: {e}")))?;
                ModelHandle::named(provider.as_str(), client.completion_model(model))
            }
            ProviderId::Groq => {
                let client = rig::providers::groq::Client::new(key)
                    .map_err(|e| AppError::Provider(format!("Failed to initialize Groq client: {e}")))?;
                ModelHandle::named(provider.as_str(), client.completion_model(model))
            }
            ProviderId::OpenRouter => {
                let client = rig::providers::openrouter::Client::new(key)
                    .map_err(|e| AppError::Provider(format!("Failed to initialize OpenRouter client: {e}")))?;
                ModelHandle::named(provider.as_str(), client.completion_model(model))
            }
            ProviderId::XAi => {
                let client = rig::providers::xai::Client::new(key)
                    .map_err(|e| AppError::Provider(format!("Failed to initialize xAI client: {e}")))?;
                ModelHandle::named(provider.as_str(), client.completion_model(model))
            }
            ProviderId::Mistral => {
                let client = rig::providers::mistral::Client::new(key)
                    .map_err(|e| AppError::Provider(format!("Failed to initialize Mistral client: {e}")))?;
                ModelHandle::named(provider.as_str(), client.completion_model(model))
            }
            ProviderId::Cohere => {
                let client = rig::providers::cohere::Client::new(key)
                    .map_err(|e| AppError::Provider(format!("Failed to initialize Cohere client: {e}")))?;
                ModelHandle::named(provider.as_str(), client.completion_model(model))
            }
            ProviderId::OllamaCloud => {
                let client = rig::providers::openai::Client::builder()
                    .api_key(key)
                    .base_url("https://ollama.com/v1")
                    .build()
                    .map_err(|e| AppError::Provider(format!("Failed to initialize Ollama Cloud client: {e}")))?;
                ModelHandle::named(provider.as_str(), client.completion_model(model))
            }
            ProviderId::Local => unreachable!(),
        };

        Ok(handle)
    }
}
