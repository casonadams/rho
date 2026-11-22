//! The one path from a model route to a running model handle, plus the live model lists.

use super::ModelCatalog;
use super::builders::{
    build_antigravity_model, build_api_key_model, build_chatgpt_model, build_claude_code_model,
    build_local_ollama_model, resolve_provider_key, validate_custom_provider_url,
};
use super::openai::OpenAiClient;
use crate::auth::AuthStore;
use crate::engine::compactor::llm::ModelHandle;
use rho_harness_core::config::{CacheRetention, Config, ProviderConfig};
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::provider::{ModelRoute, ProviderId};
use std::sync::Arc;
use tokio::sync::Mutex;

/// The facts needed to construct one builtin provider's model handle.
pub(crate) struct ModelRequest<'a> {
    pub provider: ProviderId,
    /// The auth-store key of the login to use: `provider` or `provider:account`.
    pub connection: &'a str,
    pub model: &'a str,
    /// rho's `thinking_level`; Antigravity uses it to pick the runtime variant.
    pub thinking_level: Option<&'a str>,
    /// How long Claude keeps each request's prompt cache.
    pub cache_retention: CacheRetention,
    /// The live store the built client refreshes OAuth tokens through; keys are checked against
    /// a snapshot instead, so the lock is never held while secrets resolve.
    pub shared_auth: Arc<Mutex<AuthStore>>,
}

#[derive(Clone)]
pub struct ModelRouter {
    auth: Arc<Mutex<AuthStore>>,
    catalog: ModelCatalog,
}

impl ModelRouter {
    pub fn new(auth: Arc<Mutex<AuthStore>>, catalog: ModelCatalog) -> Self {
        Self { auth, catalog }
    }

    pub fn auth(&self) -> &Arc<Mutex<AuthStore>> {
        &self.auth
    }

    pub fn catalog(&self) -> &ModelCatalog {
        &self.catalog
    }

    /// Runs `build_blocking` on the blocking pool so slow `!command` secrets never stall the runtime.
    pub async fn build(&self, config: &Config, route: &ModelRoute) -> Result<ModelHandle> {
        let (router, config, route) = (self.clone(), config.clone(), route.clone());
        tokio::task::spawn_blocking(move || router.build_blocking(&config, &route))
            .await
            .map_err(|e| AppError::Provider(format!("Model build task failed: {e}")))?
    }

    /// Builds from a snapshot of the auth store, so the auth lock is never held while secrets resolve.
    /// Must not be called from an async task.
    pub fn build_blocking(&self, config: &Config, route: &ModelRoute) -> Result<ModelHandle> {
        let auth_store = self.auth.blocking_lock().clone();
        let Some(provider) = route.connection.provider_id() else {
            return build_custom_model(config, route, &auth_store);
        };
        crate::install_crypto_provider();
        let connection = route.connection.to_string();
        let request = ModelRequest {
            provider,
            connection: &connection,
            model: &route.model,
            thinking_level: config.thinking_level.as_deref(),
            cache_retention: config.cache_retention,
            shared_auth: self.auth.clone(),
        };
        build_builtin_model(&request, &auth_store)
    }

    /// The guard model, built without thinking so its verdicts stay fast and deterministic.
    pub async fn build_guard(&self, config: &Config) -> Result<Option<ModelHandle>> {
        let Some(route) = guard_route(config) else {
            return Ok(None);
        };
        let config = Config {
            thinking_level: rho_harness_core::thinking::ThinkingLevel::Off.setting(),
            ..config.clone()
        };
        self.build(&config, &route).await.map(Some)
    }
}

pub fn guard_route(config: &Config) -> Option<ModelRoute> {
    let route = ModelRoute::parse(config.guard_model()?, None, "local");
    (!route.model.is_empty()).then_some(route)
}

fn build_builtin_model(request: &ModelRequest<'_>, auth_store: &AuthStore) -> Result<ModelHandle> {
    let (provider, connection) = (request.provider, request.connection);
    match provider {
        ProviderId::Local => build_local_ollama_model(request.model),
        ProviderId::Antigravity => Ok(build_antigravity_model(request, auth_store)),
        ProviderId::ClaudeCode => {
            resolve_provider_key(provider, connection, auth_store)?;
            Ok(build_claude_code_model(request))
        }
        ProviderId::ChatGpt => {
            resolve_provider_key(provider, connection, auth_store)?;
            Ok(build_chatgpt_model(request, auth_store))
        }
        ProviderId::Anthropic
        | ProviderId::OpenAi
        | ProviderId::OpenRouter
        | ProviderId::Gemini
        | ProviderId::Groq
        | ProviderId::DeepSeek
        | ProviderId::XAi
        | ProviderId::Mistral
        | ProviderId::Cohere
        | ProviderId::OllamaCloud => {
            build_api_key_model(request, resolve_provider_key(provider, connection, auth_store)?)
        }
    }
}

fn build_custom_model(config: &Config, route: &ModelRoute, auth_store: &AuthStore) -> Result<ModelHandle> {
    let name = route.provider();
    let spec = config.providers.get(name).ok_or_else(|| {
        AppError::Provider(format!(
            "Unknown provider '{name}'. Configure it in config.toml as\n\
                 [providers.{name}]\nbase_url = \"https://...\"\n\
                 before selecting it."
        ))
    })?;
    validate_custom_provider_url(name, &spec.base_url, config.allow_private_network)?;
    let key = custom_key(&route.connection.to_string(), spec, auth_store)?.ok_or_else(|| {
        AppError::Auth(format!(
            "Missing API key for provider '{name}'. Set {} or run 'rho login {name}'.",
            spec.key_env.as_deref().unwrap_or("its API key env var"),
        ))
    })?;
    crate::install_crypto_provider();
    Ok(Arc::new(OpenAiClient::new(&spec.base_url, key, &route.model, name)))
}

pub(crate) fn custom_key(name: &str, spec: &ProviderConfig, auth_store: &AuthStore) -> Result<Option<String>> {
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
