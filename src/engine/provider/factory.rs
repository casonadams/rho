//! Provider factory: build a rig [`ModelHandle`] for any [`ProviderId`].
//!
//! - [`ProviderFactory::create_model`] accepts a string provider name and is
//!   the entry point used by most callers.
//! - [`ProviderFactory::create_model_for`] accepts an already-parsed
//!   [`ProviderId`] and is used when the caller has already determined the
//!   provider (e.g. [`super::catalog::list_models`] internally doesn't need
//!   this, but other modules do).
//!
//! Per-provider construction is delegated to [`build_api_key_model`],
//! [`build_chatgpt_model`], [`build_copilot_model`], and [`build_ollama_model`]
//! based on the provider's [`crate::engine::provider::CredentialStrategy`].

use super::id::ProviderId;
use crate::auth::{AuthStore, chatgpt_client, copilot_client};
use crate::error::{AppError, Result};
use rig::agent::ModelHandle;
use rig::client::CompletionClient;
use std::path::Path;
use std::str::FromStr;

pub struct ProviderFactory;

#[derive(Clone, Copy)]
pub struct ModelRequest<'a> {
    pub model: &'a str,
    pub config_dir: &'a Path,
}

impl ProviderFactory {
    pub fn create_model(provider: &str, request: ModelRequest<'_>, auth_store: &AuthStore) -> Result<ModelHandle> {
        Self::create_model_for(ProviderId::from_str(provider)?, request, auth_store)
    }

    pub fn create_model_for(
        provider: ProviderId,
        request: ModelRequest<'_>,
        auth_store: &AuthStore,
    ) -> Result<ModelHandle> {
        match provider {
            ProviderId::ChatGpt => build_chatgpt_model(request.model, request.config_dir),
            ProviderId::Copilot => build_copilot_model(request.model, request.config_dir),
            ProviderId::Ollama => build_ollama_model(request.model),
            _ => build_api_key_model(provider, request.model, api_key(provider, auth_store)?),
        }
    }
}

pub(super) fn api_key(provider: ProviderId, auth_store: &AuthStore) -> Result<String> {
    auth_store.get_key(provider.as_str())?.ok_or_else(|| {
        AppError::Auth(format!(
            "{} is not set",
            provider.api_key_env().unwrap_or("provider API key")
        ))
    })
}

fn build_api_key_model(provider: ProviderId, model: &str, key: String) -> Result<ModelHandle> {
    let handle = match provider {
        ProviderId::Anthropic => model_handle(provider, rig::providers::anthropic::Client::new(key), model),
        ProviderId::OpenAi => model_handle(provider, rig::providers::openai::Client::new(key), model),
        ProviderId::DeepSeek => model_handle(provider, rig::providers::deepseek::Client::new(key), model),
        ProviderId::Gemini => model_handle(provider, rig::providers::gemini::Client::new(key), model),
        ProviderId::Groq => model_handle(provider, rig::providers::groq::Client::new(key), model),
        ProviderId::OpenRouter => model_handle(provider, rig::providers::openrouter::Client::new(key), model),
        ProviderId::XAi => model_handle(provider, rig::providers::xai::Client::new(key), model),
        ProviderId::Mistral => model_handle(provider, rig::providers::mistral::Client::new(key), model),
        ProviderId::Cohere => model_handle(provider, rig::providers::cohere::Client::new(key), model),
        ProviderId::ChatGpt | ProviderId::Copilot | ProviderId::Ollama => {
            return Err(AppError::Provider(format!(
                "{provider} does not use API-key model construction"
            )));
        }
    };
    handle.map_err(|_| AppError::Provider(format!("Failed to initialize {provider} client")))
}

fn model_handle<C, E>(
    provider: ProviderId,
    client: std::result::Result<C, E>,
    model: &str,
) -> std::result::Result<ModelHandle, E>
where
    C: CompletionClient,
    C::CompletionModel: 'static,
{
    client.map(|client| ModelHandle::named(provider.as_str(), client.completion_model(model)))
}

fn build_chatgpt_model(model: &str, config_dir: &Path) -> Result<ModelHandle> {
    let token_dir = crate::auth::OAuthManager::new(config_dir).token_dir(ProviderId::ChatGpt)?;
    let client = chatgpt_client(&token_dir, false)?;
    Ok(ModelHandle::named(
        ProviderId::ChatGpt.as_str(),
        client.completion_model(model),
    ))
}

fn build_copilot_model(model: &str, config_dir: &Path) -> Result<ModelHandle> {
    let token_dir = crate::auth::OAuthManager::new(config_dir).token_dir(ProviderId::Copilot)?;
    let client = copilot_client(&token_dir, false)?;
    Ok(ModelHandle::named(
        ProviderId::Copilot.as_str(),
        client.completion_model(model),
    ))
}

fn build_ollama_model(model: &str) -> Result<ModelHandle> {
    let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let client = rig::providers::ollama::Client::builder()
        .api_key("")
        .base_url(&host)
        .build()
        .map_err(|_| AppError::Provider("Failed to initialize ollama client".to_string()))?;
    Ok(ModelHandle::named(
        ProviderId::Ollama.as_str(),
        client.completion_model(model),
    ))
}
