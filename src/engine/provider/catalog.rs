//! Model discovery: live listings where rig supports it, curated fallbacks
//! otherwise.

use super::ProviderId;
use super::factory::api_key;
use crate::auth::{AuthStore, copilot_client};
use crate::error::{AppError, Result};
use rig::client::ModelListingClient;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelCatalog {
    Live(Vec<String>),
    Curated(Vec<&'static str>),
}

impl ModelCatalog {
    pub fn source_label(&self) -> &'static str {
        match self {
            Self::Live(_) => "live provider listing",
            Self::Curated(_) => "curated examples; not live discovery",
        }
    }

    pub fn models(&self) -> Vec<&str> {
        match self {
            Self::Live(models) => models.iter().map(String::as_str).collect(),
            Self::Curated(models) => models.clone(),
        }
    }
}

pub async fn list_models(provider: ProviderId, auth_store: &AuthStore, config_dir: &Path) -> Result<ModelCatalog> {
    match provider {
        ProviderId::Anthropic => list(rig::providers::anthropic::Client::new(api_key(provider, auth_store)?)).await,
        ProviderId::OpenAi => list(rig::providers::openai::Client::new(api_key(provider, auth_store)?)).await,
        ProviderId::DeepSeek => list(rig::providers::deepseek::Client::new(api_key(provider, auth_store)?)).await,
        ProviderId::Gemini => list(rig::providers::gemini::Client::new(api_key(provider, auth_store)?)).await,
        ProviderId::Groq => list(rig::providers::groq::Client::new(api_key(provider, auth_store)?)).await,
        ProviderId::Ollama => {
            let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string());
            list(
                rig::providers::ollama::Client::builder()
                    .api_key("")
                    .base_url(&host)
                    .build(),
            )
            .await
        }
        ProviderId::OpenRouter => list(rig::providers::openrouter::Client::new(api_key(provider, auth_store)?)).await,
        ProviderId::Mistral => list(rig::providers::mistral::Client::new(api_key(provider, auth_store)?)).await,
        ProviderId::Copilot => {
            let token_dir = crate::auth::OAuthManager::new(config_dir).token_dir(provider)?;
            let client = copilot_client(&token_dir, false)?;
            list::<_, std::convert::Infallible>(Ok(client)).await
        }
        ProviderId::ChatGpt => Ok(curated(provider)),
        ProviderId::XAi | ProviderId::Cohere => Ok(curated(provider)),
    }
}

async fn list<C, E>(client: std::result::Result<C, E>) -> Result<ModelCatalog>
where
    C: ModelListingClient,
{
    let client = client.map_err(|_| AppError::Provider("Failed to initialize model listing".to_string()))?;
    let models = client
        .list_models()
        .await
        .map_err(|_| AppError::Provider("Model listing failed without exposing provider credentials".to_string()))?;
    Ok(ModelCatalog::Live(models.into_iter().map(|model| model.id).collect()))
}

pub fn curated(provider: ProviderId) -> ModelCatalog {
    let models = match provider {
        ProviderId::Anthropic => vec!["claude-sonnet-4-6", "claude-haiku-4-5"],
        ProviderId::OpenAi => vec!["gpt-5.4", "gpt-5.4-mini"],
        ProviderId::ChatGpt => vec!["gpt-5.4", "gpt-5.3-codex"],
        ProviderId::Copilot => vec!["gpt-4.1", "gpt-5.3-codex"],
        ProviderId::DeepSeek => vec!["deepseek-chat", "deepseek-reasoner"],
        ProviderId::Gemini => vec!["gemini-2.5-pro", "gemini-2.5-flash"],
        ProviderId::Groq => vec!["llama-3.3-70b-versatile"],
        ProviderId::Ollama => vec!["local models"],
        ProviderId::OpenRouter => vec!["openrouter/auto"],
        ProviderId::XAi => vec!["grok-4"],
        ProviderId::Mistral => vec!["mistral-large-latest"],
        ProviderId::Cohere => vec!["command-r-plus"],
    };
    ModelCatalog::Curated(models)
}
