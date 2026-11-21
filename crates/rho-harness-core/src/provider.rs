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

/// A provider plus that provider's own model id, rendered as `provider/model`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRoute {
    pub provider: String,
    pub model: String,
}

impl ModelRoute {
    /// Pairs an already chosen provider with a model id, dropping a redundant `provider/` prefix.
    pub fn new(provider: &str, model: &str) -> Self {
        let provider = canonical_provider(provider);
        let model = model.trim();
        let model = match model.split_once('/') {
            Some((prefix, bare)) if canonical_provider(prefix) == provider => bare.trim(),
            _ => model,
        };
        Self {
            provider,
            model: model.to_string(),
        }
    }

    /// Resolves a user-entered spec: its `provider/` prefix, else `explicit`, else the provider
    /// inferred from the model name, else `fallback`.
    pub fn parse(spec: &str, explicit: Option<&str>, fallback: &str) -> Self {
        let spec = spec.trim().trim_start_matches('/');
        if let Some((provider, model)) = spec.split_once('/') {
            return Self {
                provider: canonical_provider(provider),
                model: model.trim().to_string(),
            };
        }
        let provider = explicit
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .or_else(|| infer_provider_for_model(spec))
            .unwrap_or(fallback);
        Self::new(provider, spec)
    }
}

impl fmt::Display for ModelRoute {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.provider, self.model)
    }
}

mod inference;
#[cfg(test)]
mod tests;
