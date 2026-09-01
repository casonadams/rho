use crate::auth::AuthStore;
use rho_core::error::{AppError, Result};
use rho_core::provider::ProviderId;
use rig::agent::ModelHandle;
use rig::client::CompletionClient;
use std::str::FromStr;

pub struct ProviderFactory;

impl ProviderFactory {
    pub fn create_model(provider: &str, model: &str, auth_store: &AuthStore) -> Result<ModelHandle> {
        let provider_id = ProviderId::from_str(provider)?;
        Self::create_model_for(provider_id, model, auth_store)
    }

    pub fn create_model_for(provider: ProviderId, model: &str, auth_store: &AuthStore) -> Result<ModelHandle> {
        if provider == ProviderId::Ollama {
            let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string());
            let client = rig::providers::ollama::Client::builder()
                .api_key("")
                .base_url(&host)
                .build()
                .map_err(|e| AppError::Provider(format!("Failed to initialize Ollama client: {e}")))?;
            return Ok(ModelHandle::named(provider.as_str(), client.completion_model(model)));
        }

        let key = auth_store.get_key(provider.as_str())?.ok_or_else(|| {
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
            ProviderId::OpenAi | ProviderId::ChatGpt | ProviderId::Copilot => {
                let client = rig::providers::openai::Client::new(key)
                    .map_err(|e| AppError::Provider(format!("Failed to initialize OpenAI client: {e}")))?;
                ModelHandle::named(provider.as_str(), client.completion_model(model))
            }
            ProviderId::Gemini | ProviderId::Antigravity => {
                let client = rig::providers::gemini::Client::new(key)
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
            ProviderId::Ollama => unreachable!(),
        };

        Ok(handle)
    }
}
