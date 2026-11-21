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

/// The static facts about one builtin provider.
#[derive(Debug)]
pub struct ProviderInfo {
    /// Canonical name: config, auth-store keys, and `provider/model` specs.
    pub name: &'static str,
    /// Older or informal names accepted on input.
    pub aliases: &'static [&'static str],
    pub display_name: &'static str,
    pub description: &'static str,
    pub credentials: CredentialStrategy,
    pub api_key_env: Option<&'static str>,
}

const fn info(
    name: &'static str,
    aliases: &'static [&'static str],
    display_name: &'static str,
    description: &'static str,
    credentials: CredentialStrategy,
    api_key_env: Option<&'static str>,
) -> ProviderInfo {
    ProviderInfo {
        name,
        aliases,
        display_name,
        description,
        credentials,
        api_key_env,
    }
}

use CredentialStrategy::{ApiKey, Local, OAuthOrApiKey, SubscriptionOAuth};

const ANTHROPIC: ProviderInfo = info(
    "anthropic",
    &[],
    "Anthropic",
    "Anthropic Claude models",
    ApiKey,
    Some("ANTHROPIC_API_KEY"),
);
const OPENAI: ProviderInfo = info(
    "openai",
    &[],
    "OpenAI",
    "OpenAI GPT & reasoning models",
    ApiKey,
    Some("OPENAI_API_KEY"),
);
const CHATGPT: ProviderInfo = info(
    "chatgpt",
    &[],
    "ChatGPT",
    "ChatGPT Plus/Pro subscription",
    SubscriptionOAuth,
    None,
);
const ANTIGRAVITY: ProviderInfo = info(
    "antigravity",
    &["google-antigravity"],
    "Google Antigravity",
    "Google Cloud Code Assist",
    SubscriptionOAuth,
    None,
);
const CLAUDE: ProviderInfo = info(
    "claude",
    &["claude-code", "claude-oauth"],
    "Claude (Subscription)",
    "Claude Pro/Max subscription",
    SubscriptionOAuth,
    None,
);
const DEEPSEEK: ProviderInfo = info(
    "deepseek",
    &[],
    "DeepSeek",
    "DeepSeek models",
    ApiKey,
    Some("DEEPSEEK_API_KEY"),
);
const GEMINI: ProviderInfo = info(
    "gemini",
    &["google"],
    "Gemini",
    "Google Gemini models",
    ApiKey,
    Some("GEMINI_API_KEY"),
);
const GROQ: ProviderInfo = info("groq", &[], "Groq", "Groq fast inference", ApiKey, Some("GROQ_API_KEY"));
const LOCAL: ProviderInfo = info(
    "local",
    &["ollama", "local-ollama"],
    "Local Ollama",
    "Models served by local Ollama",
    Local,
    None,
);
const OLLAMA_CLOUD: ProviderInfo = info(
    "ollama-cloud",
    &["ollamacloud"],
    "Ollama Cloud",
    "Hosted open models",
    ApiKey,
    Some("OLLAMA_API_KEY"),
);
const OPENROUTER: ProviderInfo = info(
    "openrouter",
    &[],
    "OpenRouter",
    "OpenRouter universal gateway",
    OAuthOrApiKey,
    Some("OPENROUTER_API_KEY"),
);
const XAI: ProviderInfo = info("xai", &[], "xAI", "xAI Grok models", ApiKey, Some("XAI_API_KEY"));
const MISTRAL: ProviderInfo = info(
    "mistral",
    &[],
    "Mistral",
    "Mistral and Codestral models",
    ApiKey,
    Some("MISTRAL_API_KEY"),
);
const COHERE: ProviderInfo = info(
    "cohere",
    &[],
    "Cohere",
    "Cohere Command models",
    ApiKey,
    Some("COHERE_API_KEY"),
);

impl ProviderId {
    /// Every builtin provider, in the order logins and pickers list them.
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

    pub fn info(self) -> &'static ProviderInfo {
        match self {
            Self::Anthropic => &ANTHROPIC,
            Self::OpenAi => &OPENAI,
            Self::ChatGpt => &CHATGPT,
            Self::Antigravity => &ANTIGRAVITY,
            Self::ClaudeCode => &CLAUDE,
            Self::DeepSeek => &DEEPSEEK,
            Self::Gemini => &GEMINI,
            Self::Groq => &GROQ,
            Self::Local => &LOCAL,
            Self::OllamaCloud => &OLLAMA_CLOUD,
            Self::OpenRouter => &OPENROUTER,
            Self::XAi => &XAI,
            Self::Mistral => &MISTRAL,
            Self::Cohere => &COHERE,
        }
    }

    pub fn as_str(self) -> &'static str {
        self.info().name
    }

    pub fn api_key_env(self) -> Option<&'static str> {
        self.info().api_key_env
    }

    pub fn auth_mode_label(self) -> &'static str {
        match self.info().credentials {
            ApiKey => "API key",
            SubscriptionOAuth => "subscription OAuth",
            OAuthOrApiKey => "OAuth or API key",
            Local => "local; no login",
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
        let value = value.trim().to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|id| id.as_str() == value || id.info().aliases.contains(&value.as_str()))
            .ok_or_else(|| AppError::Provider(format!("Unknown or unsupported AI provider: '{value}'")))
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
