//! Live model discovery: each provider's model-list API, fetched on demand.

pub mod antigravity;
pub(crate) mod fetch;
pub mod presets;

#[cfg(test)]
mod tests;

pub use antigravity::sort_models_newest_first;
pub use presets::{antigravity_preset_models, format_context_tokens};

use crate::auth::AuthStore;
use crate::claude::http::ClaudeAuth;
use rho_harness_core::config::Config;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::provider::ProviderId;
use std::str::FromStr;

const OLLAMA_CLOUD_HOST: &str = "https://ollama.com";

/// One model in a provider's list; the provider is the catalog entry it is listed under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredModel {
    pub id: String,
    pub name: String,
    pub description: String,
    pub context_tokens: Option<usize>,
}

impl DiscoveredModel {
    pub fn new(id: &str, name: &str, description: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            context_tokens: None,
        }
    }
}

/// The login used to list `provider`'s models: its default login, else any other account.
fn list_connection(auth: &AuthStore, provider: ProviderId) -> String {
    auth.logins(provider)
        .into_iter()
        .next()
        .map_or_else(|| provider.as_str().to_string(), |login| login.connection.to_string())
}

async fn require_key(auth: &mut AuthStore, provider: ProviderId) -> Result<String> {
    auth.get_key(&list_connection(auth, provider))
        .await?
        .ok_or_else(|| AppError::Auth(format!("Not logged in to {provider}. Run 'rho login {provider}'.")))
}

fn chatgpt_account_id(auth: &AuthStore, token: &str) -> Option<String> {
    auth.chatgpt_account_id(&list_connection(auth, ProviderId::ChatGpt), token)
}

async fn discover_antigravity(auth: &mut AuthStore) -> Result<Vec<DiscoveredModel>> {
    let token = require_key(auth, ProviderId::Antigravity).await?;
    let project_id = auth.antigravity_project(&list_connection(auth, ProviderId::Antigravity));
    Ok(fetch::fetch_antigravity(&token, &project_id).await)
}

async fn discover_builtin(provider: ProviderId, mut auth: AuthStore) -> Result<Vec<DiscoveredModel>> {
    match provider {
        ProviderId::Local => fetch::fetch_ollama(&crate::ollama::local::local_host(), None, "local").await,
        ProviderId::OllamaCloud => {
            let key = require_key(&mut auth, provider).await?;
            fetch::fetch_ollama(OLLAMA_CLOUD_HOST, Some(&key), provider.as_str()).await
        }
        ProviderId::Antigravity => discover_antigravity(&mut auth).await,
        ProviderId::ClaudeCode | ProviderId::Anthropic => {
            let token = require_key(&mut auth, provider).await?;
            let mode = if provider == ProviderId::ClaudeCode {
                ClaudeAuth::Login
            } else {
                ClaudeAuth::ApiKey
            };
            fetch::fetch_anthropic(fetch::ANTHROPIC_MODELS_URL, mode, &token, provider.as_str()).await
        }
        ProviderId::ChatGpt => {
            let token = require_key(&mut auth, provider).await?;
            let account_id = chatgpt_account_id(&auth, &token);
            fetch::fetch_codex(fetch::CODEX_MODELS_URL, &token, account_id.as_deref()).await
        }
        ProviderId::Gemini => {
            let key = require_key(&mut auth, provider).await?;
            fetch::fetch_gemini(openai_endpoint_models(provider)?, &key).await
        }
        other => {
            let url = openai_endpoint_models(other)?;
            let key = require_key(&mut auth, other).await?;
            fetch::fetch_openai_compatible(url, Some(&key), other.as_str()).await
        }
    }
}

fn openai_endpoint_models(provider: ProviderId) -> Result<&'static str> {
    crate::provider::builders::openai_endpoint(provider)
        .map(|endpoint| endpoint.models)
        .ok_or_else(|| AppError::Provider(format!("{provider} has no model-list endpoint")))
}

/// Lists the models `provider` currently offers, straight from its API.
pub async fn discover_models(provider: &str, config: &Config, auth: AuthStore) -> Result<Vec<DiscoveredModel>> {
    crate::install_crypto_provider();
    let provider = rho_harness_core::provider::Connection::parse(provider).provider;
    let provider = provider.as_str();
    if let Some(spec) = config.providers.get(provider) {
        let key = crate::provider::router::custom_key(provider, spec, &auth)?;
        let url = format!("{}/models", spec.base_url.trim_end_matches('/'));
        return fetch::fetch_openai_compatible(&url, key.as_deref(), provider).await;
    }
    discover_builtin(ProviderId::from_str(provider)?, auth).await
}
