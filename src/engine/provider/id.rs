//! Provider identity, credential strategy, and per-provider metadata.

use crate::error::{AppError, Result};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderId {
    Anthropic,
    OpenAi,
    ChatGpt,
    Copilot,
    DeepSeek,
    Gemini,
    Groq,
    Ollama,
    OpenRouter,
    XAi,
    Mistral,
    Cohere,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialStrategy {
    ApiKey,
    SubscriptionOAuth,
    Local,
}

impl ProviderId {
    pub const ALL: [Self; 12] = [
        Self::Anthropic,
        Self::OpenAi,
        Self::ChatGpt,
        Self::Copilot,
        Self::DeepSeek,
        Self::Gemini,
        Self::Groq,
        Self::Ollama,
        Self::OpenRouter,
        Self::XAi,
        Self::Mistral,
        Self::Cohere,
    ];

    pub const API_KEY_PROVIDERS: [Self; 10] = [
        Self::Anthropic,
        Self::OpenAi,
        Self::DeepSeek,
        Self::Gemini,
        Self::Groq,
        Self::Ollama,
        Self::OpenRouter,
        Self::XAi,
        Self::Mistral,
        Self::Cohere,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
            Self::ChatGpt => "chatgpt",
            Self::Copilot => "copilot",
            Self::DeepSeek => "deepseek",
            Self::Gemini => "gemini",
            Self::Groq => "groq",
            Self::Ollama => "ollama",
            Self::OpenRouter => "openrouter",
            Self::XAi => "xai",
            Self::Mistral => "mistral",
            Self::Cohere => "cohere",
        }
    }

    pub fn credential_strategy(self) -> CredentialStrategy {
        match self {
            Self::ChatGpt | Self::Copilot => CredentialStrategy::SubscriptionOAuth,
            Self::Ollama => CredentialStrategy::Local,
            _ => CredentialStrategy::ApiKey,
        }
    }

    pub fn auth_mode_label(self) -> &'static str {
        match self.credential_strategy() {
            CredentialStrategy::ApiKey => "API key",
            CredentialStrategy::SubscriptionOAuth => "subscription OAuth",
            CredentialStrategy::Local => "local; no login",
        }
    }

    pub(crate) fn api_key_env(self) -> Option<&'static str> {
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
            Self::ChatGpt | Self::Copilot | Self::Ollama => None,
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
            "copilot" => Ok(Self::Copilot),
            "deepseek" => Ok(Self::DeepSeek),
            "gemini" | "google" => Ok(Self::Gemini),
            "groq" => Ok(Self::Groq),
            "ollama" => Ok(Self::Ollama),
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
