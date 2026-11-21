use super::ModelRequest;
use crate::auth::AuthStore;
use crate::engine::compactor::llm::ModelHandle;
use crate::engine::quota_refresh::stored_oauth_account_id;
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

pub(super) fn resolve_provider_key(provider: ProviderId, auth_store: &AuthStore) -> Result<String> {
    auth_store.get_key_sync(provider.as_str())?.ok_or_else(|| {
        AppError::Auth(format!(
            "Missing API key for provider '{}'. Run 'rho login {}' or set {}.",
            provider.as_str(),
            provider.as_str(),
            provider.api_key_env().unwrap_or("API key")
        ))
    })
}

pub(super) fn build_chatgpt_model(request: &ModelRequest<'_>, auth_store: &AuthStore) -> ModelHandle {
    let account_id = stored_oauth_account_id(auth_store.get_credential("chatgpt")).or_else(|| {
        auth_store
            .get_key_sync("chatgpt")
            .ok()
            .flatten()
            .and_then(|k| crate::auth::oauth::extract_chatgpt_account_id(&k))
    });
    Arc::new(
        crate::chatgpt::ChatGptClient::with_auth_store(request.shared_auth.clone(), request.model)
            .with_account_id(account_id),
    )
}

pub(super) fn build_antigravity_model(request: &ModelRequest<'_>, auth_store: &AuthStore) -> ModelHandle {
    let project_id = stored_oauth_account_id(auth_store.get_credential("antigravity"))
        .unwrap_or_else(|| crate::auth::antigravity::stable_project_id("antigravity-default"));
    Arc::new(
        crate::antigravity::AntigravityClient::with_auth_store(request.shared_auth.clone(), project_id, request.model)
            .with_effort(request.thinking_level),
    )
}

pub(super) fn build_claude_code_model(request: &ModelRequest<'_>) -> ModelHandle {
    Arc::new(
        crate::claude::ClaudeClient::with_auth_store(request.shared_auth.clone(), request.model)
            .with_thinking_level(request.thinking_level),
    )
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

pub(super) fn build_api_key_model(provider: ProviderId, model: &str, key: String) -> Result<ModelHandle> {
    if provider == ProviderId::Anthropic {
        let model = crate::claude::request::normalize_model_alias(model);
        return Ok(Arc::new(crate::claude::ClaudeClient::with_api_key(key, model)));
    }
    let endpoint = openai_endpoint(provider)
        .ok_or_else(|| AppError::Provider(format!("Unsupported API-key provider '{provider}'")))?;
    Ok(openai_model(endpoint.chat, key, model, provider.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_api_key_provider_builds_and_reports_its_own_name() {
        crate::install_crypto_provider();
        for provider in ProviderId::ALL {
            let built = build_api_key_model(provider, "m", "key".into());
            if provider.api_key_env().is_some() {
                assert_eq!(built.unwrap().provider_name(), provider.as_str());
            } else {
                assert!(built.is_err(), "{provider}");
            }
        }
    }
}
