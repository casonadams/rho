//! Provider identity, credential strategy, and per-provider metadata.

use crate::error::{AppError, Result};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderId {
    Anthropic,
    OpenAi,
    ChatGpt,
    Antigravity,
    ClaudeCode,
    DeepSeek,
    Gemini,
    Groq,
    Local,
    OllamaCloud,
    OpenRouter,
    XAi,
    Mistral,
    Cohere,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialStrategy {
    ApiKey,
    SubscriptionOAuth,
    OAuthOrApiKey,
    Local,
}

impl ProviderId {
    pub const ALL: [Self; 14] = [
        Self::ChatGpt,
        Self::Antigravity,
        Self::ClaudeCode,
        Self::OpenRouter,
        Self::Anthropic,
        Self::OpenAi,
        Self::DeepSeek,
        Self::Gemini,
        Self::Groq,
        Self::XAi,
        Self::Mistral,
        Self::Cohere,
        Self::OllamaCloud,
        Self::Local,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
            Self::ChatGpt => "chatgpt",
            Self::Antigravity => "antigravity",
            Self::ClaudeCode => "claude",
            Self::DeepSeek => "deepseek",
            Self::Gemini => "gemini",
            Self::Groq => "groq",
            Self::Local => "local",
            Self::OllamaCloud => "ollama-cloud",
            Self::OpenRouter => "openrouter",
            Self::XAi => "xai",
            Self::Mistral => "mistral",
            Self::Cohere => "cohere",
        }
    }

    pub fn credential_strategy(self) -> CredentialStrategy {
        match self {
            Self::ChatGpt | Self::Antigravity | Self::ClaudeCode => CredentialStrategy::SubscriptionOAuth,
            Self::OpenRouter => CredentialStrategy::OAuthOrApiKey,
            Self::Local => CredentialStrategy::Local,
            _ => CredentialStrategy::ApiKey,
        }
    }

    pub fn auth_mode_label(self) -> &'static str {
        match self.credential_strategy() {
            CredentialStrategy::ApiKey => "API key",
            CredentialStrategy::SubscriptionOAuth => "subscription OAuth",
            CredentialStrategy::OAuthOrApiKey => "OAuth or API key",
            CredentialStrategy::Local => "local; no login",
        }
    }

    pub fn api_key_env(self) -> Option<&'static str> {
        match self {
            Self::Anthropic => Some("ANTHROPIC_API_KEY"),
            Self::OpenAi => Some("OPENAI_API_KEY"),
            Self::DeepSeek => Some("DEEPSEEK_API_KEY"),
            Self::Gemini => Some("GEMINI_API_KEY"),
            Self::Groq => Some("GROQ_API_KEY"),
            Self::OpenRouter => Some("OPENROUTER_API_KEY"),
            Self::XAi => Some("XAI_API_KEY"),
            Self::Mistral => Some("MISTRAL_API_KEY"),
            Self::Cohere => Some("COHERE_API_KEY"),
            Self::OllamaCloud => Some("OLLAMA_API_KEY"),
            Self::ChatGpt | Self::Antigravity | Self::ClaudeCode | Self::Local => None,
        }
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ProviderId {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "anthropic" => Ok(Self::Anthropic),
            "openai" => Ok(Self::OpenAi),
            "chatgpt" => Ok(Self::ChatGpt),
            "antigravity" | "google-antigravity" => Ok(Self::Antigravity),
            "claude" | "claude-code" | "claude-oauth" => Ok(Self::ClaudeCode),
            "deepseek" => Ok(Self::DeepSeek),
            "gemini" | "google" => Ok(Self::Gemini),
            "groq" => Ok(Self::Groq),
            "local" | "ollama" | "local-ollama" => Ok(Self::Local),
            "ollama-cloud" | "ollamacloud" => Ok(Self::OllamaCloud),
            "openrouter" => Ok(Self::OpenRouter),
            "xai" => Ok(Self::XAi),
            "mistral" => Ok(Self::Mistral),
            "cohere" => Ok(Self::Cohere),
            other => Err(AppError::Provider(format!(
                "Unknown or unsupported AI provider: '{other}'"
            ))),
        }
    }
}

pub use inference::infer_provider_for_model;

/// Builtin providers fold onto their canonical name; custom provider names are lowercased.
pub fn canonical_provider(name: &str) -> String {
    ProviderId::from_str(name).map_or_else(|_| name.trim().to_ascii_lowercase(), |id| id.as_str().to_string())
}

/// One login to a provider: `provider` (its default login) or `provider:account` (the lowercased
/// email for subscriptions). The rendering is the login's auth-store key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Connection {
    pub provider: String,
    pub account: Option<String>,
}

impl Connection {
    pub fn parse(connection: &str) -> Self {
        let (provider, account) = match connection.split_once(':') {
            Some((provider, account)) => (provider, Some(account)),
            None => (connection, None),
        };
        Self {
            provider: canonical_provider(provider),
            account: account.map(|a| a.trim().to_ascii_lowercase()).filter(|a| !a.is_empty()),
        }
    }

    /// An additional account login to a builtin provider.
    pub fn account(provider: ProviderId, account: &str) -> Self {
        Self::parse(&format!("{provider}:{account}"))
    }

    /// `None` for custom providers.
    pub fn provider_id(&self) -> Option<ProviderId> {
        ProviderId::from_str(&self.provider).ok()
    }
}

impl fmt::Display for Connection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.account {
            Some(account) => write!(formatter, "{}:{account}", self.provider),
            None => formatter.write_str(&self.provider),
        }
    }
}

/// A provider login plus that provider's own model id, rendered as `connection/model`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRoute {
    pub connection: Connection,
    pub model: String,
}

impl ModelRoute {
    /// Pairs an already chosen connection with a model id, dropping a redundant `connection/` prefix.
    pub fn new(connection: &str, model: &str) -> Self {
        let connection = Connection::parse(connection);
        let model = model.trim();
        let model = match model.split_once('/') {
            Some((prefix, bare)) if Connection::parse(prefix) == connection => bare.trim(),
            _ => model,
        };
        Self {
            connection,
            model: model.to_string(),
        }
    }

    /// Resolves a user-entered spec: its `connection/` prefix, else `explicit`, else the provider
    /// inferred from the model name, else `fallback`. An inferred provider keeps `fallback`'s
    /// account when both name the same provider.
    pub fn parse(spec: &str, explicit: Option<&str>, fallback: &str) -> Self {
        let spec = spec.trim().trim_start_matches('/');
        if let Some((connection, model)) = spec.split_once('/') {
            return Self {
                connection: Connection::parse(connection),
                model: model.trim().to_string(),
            };
        }
        if let Some(explicit) = explicit.map(str::trim).filter(|p| !p.is_empty()) {
            return Self::new(explicit, spec);
        }
        let fallback = Connection::parse(fallback);
        let connection = match infer_provider_for_model(spec) {
            Some(inferred) if canonical_provider(inferred) != fallback.provider => Connection::parse(inferred),
            _ => fallback,
        };
        Self::new(&connection.to_string(), spec)
    }

    pub fn provider(&self) -> &str {
        &self.connection.provider
    }
}

impl fmt::Display for ModelRoute {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.connection, self.model)
    }
}

mod inference;
#[cfg(test)]
mod tests;
