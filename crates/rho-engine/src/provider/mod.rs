mod builders;
pub mod capabilities;
pub mod discovery;
pub mod store;

pub use capabilities::supports_tool_result_images;
pub use store::ModelStore;

use crate::auth::AuthStore;
use builders::{
    SHARED_HTTP_CLIENT, build_antigravity_model, build_chatgpt_model, build_claude_code_model, build_gemini_model,
    build_local_ollama_model, build_standard_client_model, resolve_provider_key, validate_custom_provider_url,
};
use rho_harness_core::config::Config;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::provider::ProviderId;
use rig::agent::ModelHandle;
use rig::client::CompletionClient;
use std::str::FromStr;

#[cfg(test)]
mod tests;

/// The facts needed to construct one provider model handle.
pub struct ModelRequest<'a> {
    pub provider: ProviderId,
    pub model: &'a str,
    /// rho's `thinking_level`; Antigravity uses it to pick the runtime variant.
    pub thinking_level: Option<&'a str>,
    pub shared_auth: Option<std::sync::Arc<tokio::sync::Mutex<AuthStore>>>,
}

pub struct ProviderFactory;

impl ProviderFactory {
    pub fn create_model(config: &Config, model: &str, auth_store: &AuthStore) -> Result<ModelHandle> {
        let name = config.provider.trim();
        if let Ok(provider_id) = ProviderId::from_str(name) {
            return Self::create_model_for(
                ModelRequest {
                    provider: provider_id,
                    model,
                    thinking_level: config.thinking_level.as_deref(),
                    shared_auth: None,
                },
                auth_store,
            );
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

        validate_custom_provider_url(name, &spec.base_url, config.allow_private_network)?;

        let key = Self::custom_key(name, spec, auth_store)?.ok_or_else(|| {
            AppError::Auth(format!(
                "Missing API key for provider '{name}'. Set {} or run 'rho login {}'.",
                spec.key_env.as_deref().unwrap_or("its API key env var"),
                name
            ))
        })?;

        let client = rig::providers::openai::Client::builder()
            .http_client(SHARED_HTTP_CLIENT.clone())
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
            return Ok(build_claude_code_model(&request, auth_store));
        }
        let key = resolve_provider_key(provider, auth_store)?;
        if provider == ProviderId::ChatGpt {
            return build_chatgpt_model(model, key, auth_store);
        }
        if provider == ProviderId::Gemini {
            return build_gemini_model(model, key);
        }
        build_standard_client_model(provider, model, key)
    }
}
