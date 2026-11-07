use super::ModelRequest;
use crate::auth::AuthStore;
use crate::engine::compactor::llm::ModelHandle;
use crate::engine::quota::stored_oauth_account_id;
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
    let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string());
    crate::install_crypto_provider();
    Ok(openai_model(&format!("{host}/v1"), "", model, "ollama-local"))
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

fn build_authed_model<C>(
    request: &ModelRequest<'_>,
    make: impl FnOnce(std::sync::Arc<tokio::sync::Mutex<AuthStore>>, &str) -> C,
    into_handle: fn(C) -> ModelHandle,
) -> ModelHandle {
    into_handle(make(request.shared_auth.clone(), request.model))
}

pub(super) fn build_chatgpt_model(request: &ModelRequest<'_>, auth_store: &AuthStore) -> ModelHandle {
    let account_id = stored_oauth_account_id(auth_store.get_credential("chatgpt")).or_else(|| {
        auth_store
            .get_key_sync("chatgpt")
            .ok()
            .flatten()
            .and_then(|k| crate::auth::oauth::extract_chatgpt_account_id(&k))
    });
    build_authed_model(
        request,
        |store, model| crate::chatgpt::ChatGptClient::with_auth_store(store, model).with_account_id(account_id),
        crate::chatgpt::into_handle,
    )
}

pub(super) fn build_gemini_model(model: &str, key: String) -> Result<ModelHandle> {
    Ok(openai_model(
        "https://generativelanguage.googleapis.com/v1beta/openai",
        key,
        model,
        "gemini",
    ))
}

pub(super) fn build_antigravity_model(request: &ModelRequest<'_>, auth_store: &AuthStore) -> ModelHandle {
    let project_id = stored_oauth_account_id(auth_store.get_credential("antigravity"))
        .unwrap_or_else(|| crate::auth::antigravity::stable_project_id("antigravity-default"));
    build_authed_model(
        request,
        |store, model| {
            crate::antigravity::AntigravityClient::with_auth_store(store, project_id, model)
                .with_effort(request.thinking_level)
        },
        crate::antigravity::into_handle,
    )
}

pub(super) fn build_claude_code_model(request: &ModelRequest<'_>) -> ModelHandle {
    build_authed_model(
        request,
        |store, model| {
            crate::claude::ClaudeClient::with_auth_store(store, model).with_thinking_level(request.thinking_level)
        },
        crate::claude::into_handle,
    )
}

fn openai_model(base_url: &str, key: impl Into<String>, model: &str, provider: &str) -> ModelHandle {
    Arc::new(OpenAiClient::new(base_url, key, model, provider))
}

fn build_rig_named_client(provider: ProviderId, model: &str, key: String) -> Result<ModelHandle> {
    match provider {
        ProviderId::Anthropic => {
            let model = crate::claude::request::normalize_model_alias(model);
            Ok(Arc::new(crate::claude::ClaudeClient::with_api_key(key, model)))
        }
        ProviderId::DeepSeek => Ok(openai_model("https://api.deepseek.com", key, model, "deepseek")),
        ProviderId::Groq => Ok(openai_model("https://api.groq.com/openai/v1", key, model, "groq")),
        ProviderId::OpenRouter => Ok(openai_model("https://openrouter.ai/api/v1", key, model, "openrouter")),
        ProviderId::XAi => Ok(openai_model("https://api.x.ai/v1", key, model, "xai")),
        ProviderId::Mistral => Ok(openai_model("https://api.mistral.ai/v1", key, model, "mistral")),
        ProviderId::Cohere => Ok(openai_model("https://api.cohere.com/v2", key, model, "cohere")),
        _ => Err(AppError::Provider(format!(
            "Unsupported standard provider '{}'",
            provider.as_str()
        ))),
    }
}

fn build_ollama_cloud_model(model: &str, key: String) -> Result<ModelHandle> {
    crate::install_crypto_provider();
    Ok(openai_model("https://ollama.com/v1", key, model, "ollama-cloud"))
}

pub(super) fn build_standard_client_model(provider: ProviderId, model: &str, key: String) -> Result<ModelHandle> {
    match provider {
        ProviderId::OpenAi => Ok(openai_model("https://api.openai.com/v1", key, model, "openai")),
        ProviderId::OllamaCloud => build_ollama_cloud_model(model, key),
        _ => build_rig_named_client(provider, model, key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_rig_named_client_variants() {
        crate::install_crypto_provider();
        assert!(build_rig_named_client(ProviderId::Anthropic, "claude-3", "key".into()).is_ok());
        assert!(build_rig_named_client(ProviderId::DeepSeek, "deepseek-chat", "key".into()).is_ok());
        assert!(build_rig_named_client(ProviderId::Groq, "llama-3", "key".into()).is_ok());
        assert!(build_rig_named_client(ProviderId::OpenRouter, "model", "key".into()).is_ok());
        assert!(build_rig_named_client(ProviderId::XAi, "grok-1", "key".into()).is_ok());
        assert!(build_rig_named_client(ProviderId::Mistral, "mistral-large", "key".into()).is_ok());
        assert!(build_rig_named_client(ProviderId::Cohere, "command-r", "key".into()).is_ok());
        assert!(build_rig_named_client(ProviderId::Local, "m", "key".into()).is_err());
    }
}
