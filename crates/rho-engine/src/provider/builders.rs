use super::router::ModelRequest;
use crate::auth::AuthStore;
use crate::auth::token::{AuthStoreTokenProvider, TokenProvider};
use crate::engine::compactor::llm::ModelHandle;
use crate::provider::openai::OpenAiClient;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::provider::ProviderId;
use std::sync::Arc;

pub(super) fn validate_custom_provider_url(name: &str, url: &str, allow_private: bool) -> Result<()> {
    rho_harness_core::net::validate_url(url, allow_private)
        .map(|_| ())
        .map_err(|e| match e {
            AppError::Tool(message) => AppError::Provider(format!("Provider '{name}': {message}")),
            other => other,
        })
}

pub(super) fn build_local_ollama_model(model: &str) -> Result<ModelHandle> {
    crate::install_crypto_provider();
    let host = crate::ollama::local::local_host();
    Ok(openai_model(
        &format!("{host}/v1"),
        "",
        model,
        ProviderId::Local.as_str(),
    ))
}

pub(super) fn resolve_provider_key(provider: ProviderId, connection: &str, auth_store: &AuthStore) -> Result<String> {
    auth_store.get_key_sync(connection)?.ok_or_else(|| {
        AppError::Auth(format!(
            "Missing API key for provider '{connection}'. Run 'rho login {}' or set {}.",
            provider.as_str(),
            provider.api_key_env().unwrap_or("API key")
        ))
    })
}

fn stored_tokens(request: &ModelRequest<'_>) -> Arc<dyn TokenProvider> {
    Arc::new(AuthStoreTokenProvider::new(
        request.shared_auth.clone(),
        request.connection,
    ))
}

pub(super) fn build_chatgpt_model(request: &ModelRequest<'_>, auth_store: &AuthStore) -> ModelHandle {
    let token = auth_store
        .get_key_sync(request.connection)
        .ok()
        .flatten()
        .unwrap_or_default();
    let account_id = auth_store.chatgpt_account_id(request.connection, &token);
    Arc::new(
        crate::chatgpt::ChatGptClient::with_token_provider(stored_tokens(request), request.model)
            .with_account_id(account_id),
    )
}

pub(super) fn build_antigravity_model(request: &ModelRequest<'_>, auth_store: &AuthStore) -> ModelHandle {
    let project_id = auth_store.antigravity_project(request.connection);
    Arc::new(
        crate::antigravity::AntigravityClient::with_token_provider(stored_tokens(request), project_id, request.model)
            .with_effort(request.thinking_level),
    )
}

pub(super) fn build_claude_code_model(request: &ModelRequest<'_>) -> ModelHandle {
    Arc::new(configure_claude(
        crate::claude::ClaudeClient::with_token_provider(stored_tokens(request), request.model),
        request,
    ))
}

fn configure_claude(client: crate::claude::ClaudeClient, request: &ModelRequest<'_>) -> crate::claude::ClaudeClient {
    client
        .with_thinking_level(request.thinking_level)
        .with_cache_retention(request.cache_retention)
}

fn openai_model(base_url: &str, key: impl Into<String>, model: &str, provider: &str) -> ModelHandle {
    Arc::new(OpenAiClient::new(base_url, key, model, provider))
}

/// Chat and model-list URLs of a builtin provider served through `OpenAiClient`.
pub(crate) struct OpenAiEndpoint {
    pub chat: &'static str,
    pub models: &'static str,
}

pub(crate) fn openai_endpoint(provider: ProviderId) -> Option<OpenAiEndpoint> {
    let (chat, models) = match provider {
        ProviderId::OpenAi => ("https://api.openai.com/v1", "https://api.openai.com/v1/models"),
        ProviderId::OpenRouter => ("https://openrouter.ai/api/v1", "https://openrouter.ai/api/v1/models"),
        ProviderId::Groq => (
            "https://api.groq.com/openai/v1",
            "https://api.groq.com/openai/v1/models",
        ),
        ProviderId::DeepSeek => ("https://api.deepseek.com", "https://api.deepseek.com/models"),
        ProviderId::XAi => ("https://api.x.ai/v1", "https://api.x.ai/v1/models"),
        ProviderId::Mistral => ("https://api.mistral.ai/v1", "https://api.mistral.ai/v1/models"),
        ProviderId::OllamaCloud => ("https://ollama.com/v1", "https://ollama.com/v1/models"),
        ProviderId::Cohere => (
            "https://api.cohere.ai/compatibility/v1",
            "https://api.cohere.com/v1/models?endpoint=chat&page_size=1000",
        ),
        ProviderId::Gemini => (
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "https://generativelanguage.googleapis.com/v1beta/models?pageSize=1000",
        ),
        _ => return None,
    };
    Some(OpenAiEndpoint { chat, models })
}

pub(super) fn build_api_key_model(request: &ModelRequest<'_>, key: String) -> Result<ModelHandle> {
    let (provider, model) = (request.provider, request.model);
    if provider == ProviderId::Anthropic {
        let model = crate::claude::request::normalize_model_alias(model);
        let client = crate::claude::ClaudeClient::with_api_key(key, model);
        return Ok(Arc::new(configure_claude(client, request)));
    }
    let endpoint = openai_endpoint(provider)
        .ok_or_else(|| AppError::Provider(format!("Unsupported API-key provider '{provider}'")))?;
    Ok(openai_model(endpoint.chat, key, model, provider.as_str()))
}

#[cfg(test)]
pub(super) fn test_request(provider: ProviderId, model: &'static str) -> ModelRequest<'static> {
    ModelRequest {
        provider,
        connection: provider.as_str(),
        model,
        thinking_level: None,
        cache_retention: rho_harness_core::config::CacheRetention::Short,
        shared_auth: Arc::new(tokio::sync::Mutex::new(AuthStore::default())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_api_key_provider_builds_and_reports_its_own_name() {
        crate::install_crypto_provider();
        for provider in ProviderId::ALL {
            let built = build_api_key_model(&test_request(provider, "m"), "key".into());
            if provider.api_key_env().is_some() {
                assert_eq!(built.unwrap().provider_name(), provider.as_str());
            } else {
                assert!(built.is_err(), "{provider}");
            }
        }
    }
}
