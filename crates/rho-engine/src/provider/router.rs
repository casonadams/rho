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

    /// Builds a structured [`crate::decision::DecisionHandle`] for `route`, preferring native
    /// decision APIs (`gemini/gemini-decision-1`, `typesafe/jev-latest`, `openrouter/*decision*`)
    /// and wrapping chat models in [`crate::decision::ChatDecisionAdapter`].
    pub async fn build_decision(&self, config: &Config, route: &ModelRoute) -> Result<crate::decision::DecisionHandle> {
        if crate::decision::is_native_decision_model(route.provider(), &route.model) {
            return self.build_native_decision(route).await;
        }
        let config = Config {
            thinking_level: rho_harness_core::thinking::ThinkingLevel::Off.setting(),
            ..config.clone()
        };
        let model = self.build(&config, route).await?;
        Ok(Arc::new(crate::decision::ChatDecisionAdapter::new(model)))
    }

    /// Resolves the first configured role in `role_chain` as a [`crate::decision::DecisionHandle`].
    pub async fn build_role_decision(
        &self,
        config: &Config,
        role_chain: &[&str],
    ) -> Result<Option<crate::decision::DecisionHandle>> {
        let Some((_, spec)) = config.resolve_role_chain(role_chain) else {
            return Ok(None);
        };
        let route = ModelRoute::parse(spec, None, "local");
        if route.model.is_empty() {
            return Ok(None);
        }
        self.build_decision(config, &route).await.map(Some)
    }

    /// The guard model resolved as a [`crate::decision::DecisionHandle`].
    pub async fn build_guard_decision(&self, config: &Config) -> Result<Option<crate::decision::DecisionHandle>> {
        self.build_role_decision(config, &["guard"]).await
    }

    async fn build_native_decision(&self, route: &ModelRoute) -> Result<crate::decision::DecisionHandle> {
        crate::install_crypto_provider();
        let mut auth_store = self.auth.lock().await.clone();
        let connection = route.connection.to_string();
        let provider = route.provider().to_ascii_lowercase();
        match provider.as_str() {
            "gemini" | "google" => {
                let key = resolve_provider_key(ProviderId::Gemini, &connection, &auth_store)?;
                Ok(Arc::new(crate::decision::GeminiDecisionClient::new(key, &route.model)))
            }
            "typesafe" => {
                let key = resolve_typesafe_key(&connection, &mut auth_store).await?;
                Ok(Arc::new(crate::decision::SystemOneClient::typesafe(key, &route.model)))
            }
            "openrouter" => {
                let key = resolve_provider_key(ProviderId::OpenRouter, &connection, &auth_store)?;
                Ok(Arc::new(crate::decision::SystemOneClient::openrouter(
                    key,
                    &route.model,
                )))
            }
            _ => Err(AppError::Provider(format!(
                "Unsupported native decision provider '{}'",
                route.provider()
            ))),
        }
    }
}

async fn resolve_typesafe_key(connection: &str, auth_store: &mut AuthStore) -> Result<String> {
    if let Ok(key) = std::env::var("TYPESAFE_API_KEY")
        && !key.trim().is_empty()
    {
        return Ok(key.trim().to_string());
    }
    auth_store.get_key(connection).await?.ok_or_else(|| {
        AppError::Auth(
            "Missing API key for provider 'typesafe'. Set TYPESAFE_API_KEY or run 'rho login typesafe'.".to_string(),
        )
    })
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
