pub mod adapter;
mod builders;
pub mod catalog;
pub mod discovery;
pub mod error;
pub mod openai;
pub mod sse;

pub use adapter::{ModelAdapter, ModelCompletionRequest, ModelCompletionResponse, ModelStreamEvent};

pub use builders::LOCAL_OLLAMA_PROVIDER;
pub use catalog::{ModelCatalog, ProviderModels, catalog_key};
pub use discovery::DiscoveredModel;
pub use openai::OpenAiClient;

use crate::auth::AuthStore;
pub use crate::engine::compactor::llm::ModelHandle;
use builders::{
    build_antigravity_model, build_chatgpt_model, build_claude_code_model, build_gemini_model,
    build_local_ollama_model, build_standard_client_model, resolve_provider_key, validate_custom_provider_url,
};
use rho_harness_core::config::Config;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::provider::{ModelRoute, ProviderId};
use std::str::FromStr;

#[cfg(test)]
mod tests;

/// The facts needed to construct one provider model handle.
pub struct ModelRequest<'a> {
    pub provider: ProviderId,
    pub model: &'a str,
    /// rho's `thinking_level`; Antigravity uses it to pick the runtime variant.
    pub thinking_level: Option<&'a str>,
    pub shared_auth: std::sync::Arc<tokio::sync::Mutex<AuthStore>>,
}

pub struct ProviderFactory;

pub fn resolve_guard_model(config: &Config) -> Option<(ProviderId, String)> {
    let route = ModelRoute::parse(config.guard_model()?, None, "local");
    if route.model.is_empty() {
        return None;
    }
    Some((ProviderId::from_str(&route.provider).ok()?, route.model))
}

impl ProviderFactory {
    pub fn guard_model_request<'a>(
        provider: ProviderId,
        model: &'a str,
        shared_auth: std::sync::Arc<tokio::sync::Mutex<AuthStore>>,
    ) -> ModelRequest<'a> {
        // Guard models never think to guarantee fast and deterministic evaluation.
        ModelRequest {
            provider,
            model,
            thinking_level: None,
            shared_auth,
        }
    }

    pub fn create_guard_model(
        config: &Config,
        auth_store: &AuthStore,
        shared_auth: std::sync::Arc<tokio::sync::Mutex<AuthStore>>,
    ) -> Result<Option<ModelHandle>> {
        let Some((provider, model)) = resolve_guard_model(config) else {
            return Ok(None);
        };
        let request = Self::guard_model_request(provider, &model, shared_auth);
        let handle = Self::create_model_for(request, auth_store)?;
        Ok(Some(handle))
    }

    pub fn create_model(
        config: &Config,
        route: &ModelRoute,
        auth_store: &AuthStore,
        shared_auth: std::sync::Arc<tokio::sync::Mutex<AuthStore>>,
    ) -> Result<ModelHandle> {
        let Ok(provider) = ProviderId::from_str(&route.provider) else {
            return Self::create_custom_model(config, &route.provider, &route.model, auth_store);
        };
        Self::create_model_for(
            ModelRequest {
                provider,
                model: &route.model,
                thinking_level: config.thinking_level.as_deref(),
                shared_auth,
            },
            auth_store,
        )
    }

    fn create_custom_model(
        config: &Config,
        provider_name: &str,
        model: &str,
        auth_store: &AuthStore,
    ) -> Result<ModelHandle> {
        let name = provider_name.trim();
        let spec = config.providers.get(&name.to_ascii_lowercase()).ok_or_else(|| {
            AppError::Provider(format!(
                "Unknown provider '{name}'. Configure it in config.toml as\n\
                     [providers.{name}]\nbase_url = \"https://...\"\n\
                     before selecting it."
            ))
        })?;

        validate_custom_provider_url(name, &spec.base_url, config.allow_private_network)?;

        let key = Self::custom_key(name, spec, auth_store)?.ok_or_else(|| {
            AppError::Auth(format!(
                "Missing API key for provider '{name}'. Set {} or run 'rho login {}'.",
                spec.key_env.as_deref().unwrap_or("its API key env var"),
                name
            ))
        })?;

        crate::install_crypto_provider();
        let client = OpenAiClient::new(&spec.base_url, key, model, name);
        Ok(std::sync::Arc::new(client))
    }

    pub(crate) fn custom_key(
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

    pub fn create_model_for(request: ModelRequest<'_>, auth_store: &AuthStore) -> Result<ModelHandle> {
        crate::install_crypto_provider();
        let (provider, model) = (request.provider, request.model);
        if provider == ProviderId::Local {
            return build_local_ollama_model(model);
        }
        if provider == ProviderId::Antigravity {
            return Ok(build_antigravity_model(&request, auth_store));
        }
        if provider == ProviderId::ClaudeCode {
            let _ = resolve_provider_key(provider, auth_store)?;
            return Ok(build_claude_code_model(&request));
        }
        let key = resolve_provider_key(provider, auth_store)?;
        if provider == ProviderId::ChatGpt {
            return Ok(build_chatgpt_model(&request, auth_store));
        }
        if provider == ProviderId::Gemini {
            return build_gemini_model(model, key);
        }
        build_standard_client_model(provider, model, key)
    }
}
