use crate::auth::{ApiKeyVerifier, AuthStore, VerificationStatus, chatgpt_client, copilot_client};
use crate::error::{AppError, Result};
use rig::agent::ModelHandle;
use rig::client::{CompletionClient, ModelListingClient, VerifyClient};
use std::fmt;
use std::path::Path;
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

pub struct ProviderFactory;

#[derive(Clone, Copy)]
pub struct ModelRequest<'a> {
    pub model: &'a str,
    pub config_dir: &'a Path,
}

impl ProviderFactory {
    pub fn create_model(provider: &str, request: ModelRequest<'_>, auth_store: &AuthStore) -> Result<ModelHandle> {
        Self::create_model_for(ProviderId::from_str(provider)?, request, auth_store)
    }

    pub fn create_model_for(
        provider: ProviderId,
        request: ModelRequest<'_>,
        auth_store: &AuthStore,
    ) -> Result<ModelHandle> {
        match provider {
            ProviderId::ChatGpt => build_chatgpt_model(request.model, request.config_dir),
            ProviderId::Copilot => build_copilot_model(request.model, request.config_dir),
            ProviderId::Ollama => build_ollama_model(request.model),
            _ => build_api_key_model(provider, request.model, api_key(provider, auth_store)?),
        }
    }
}

fn api_key(provider: ProviderId, auth_store: &AuthStore) -> Result<String> {
    auth_store.get_key(provider.as_str())?.ok_or_else(|| {
        AppError::Auth(format!(
            "{} is not set",
            provider.api_key_env().unwrap_or("provider API key")
        ))
    })
}

fn build_api_key_model(provider: ProviderId, model: &str, key: String) -> Result<ModelHandle> {
    let handle = match provider {
        ProviderId::Anthropic => model_handle(provider, rig::providers::anthropic::Client::new(key), model),
        ProviderId::OpenAi => model_handle(provider, rig::providers::openai::Client::new(key), model),
        ProviderId::DeepSeek => model_handle(provider, rig::providers::deepseek::Client::new(key), model),
        ProviderId::Gemini => model_handle(provider, rig::providers::gemini::Client::new(key), model),
        ProviderId::Groq => model_handle(provider, rig::providers::groq::Client::new(key), model),
        ProviderId::OpenRouter => model_handle(provider, rig::providers::openrouter::Client::new(key), model),
        ProviderId::XAi => model_handle(provider, rig::providers::xai::Client::new(key), model),
        ProviderId::Mistral => model_handle(provider, rig::providers::mistral::Client::new(key), model),
        ProviderId::Cohere => model_handle(provider, rig::providers::cohere::Client::new(key), model),
        ProviderId::ChatGpt | ProviderId::Copilot | ProviderId::Ollama => {
            return Err(AppError::Provider(format!(
                "{provider} does not use API-key model construction"
            )));
        }
    };
    handle.map_err(|_| AppError::Provider(format!("Failed to initialize {provider} client")))
}

fn model_handle<C, E>(
    provider: ProviderId,
    client: std::result::Result<C, E>,
    model: &str,
) -> std::result::Result<ModelHandle, E>
where
    C: CompletionClient,
    C::CompletionModel: 'static,
{
    client.map(|client| ModelHandle::named(provider.as_str(), client.completion_model(model)))
}

fn build_chatgpt_model(model: &str, config_dir: &Path) -> Result<ModelHandle> {
    let token_dir = crate::auth::OAuthManager::new(config_dir).token_dir(ProviderId::ChatGpt)?;
    let client = chatgpt_client(&token_dir, false)?;
    Ok(ModelHandle::named(
        ProviderId::ChatGpt.as_str(),
        client.completion_model(model),
    ))
}

fn build_copilot_model(model: &str, config_dir: &Path) -> Result<ModelHandle> {
    let token_dir = crate::auth::OAuthManager::new(config_dir).token_dir(ProviderId::Copilot)?;
    let client = copilot_client(&token_dir, false)?;
    Ok(ModelHandle::named(
        ProviderId::Copilot.as_str(),
        client.completion_model(model),
    ))
}

fn build_ollama_model(model: &str) -> Result<ModelHandle> {
    let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let client = rig::providers::ollama::Client::builder()
        .api_key("")
        .base_url(&host)
        .build()
        .map_err(|_| AppError::Provider("Failed to initialize ollama client".to_string()))?;
    Ok(ModelHandle::named(
        ProviderId::Ollama.as_str(),
        client.completion_model(model),
    ))
}

pub struct RigCredentialVerifier;

#[async_trait::async_trait]
impl ApiKeyVerifier for RigCredentialVerifier {
    async fn verify(&self, provider: ProviderId, key: &str) -> Result<VerificationStatus> {
        match provider {
            ProviderId::Anthropic => verify(rig::providers::anthropic::Client::new(key)).await,
            ProviderId::OpenAi => verify(rig::providers::openai::Client::new(key)).await,
            ProviderId::DeepSeek => verify(rig::providers::deepseek::Client::new(key)).await,
            ProviderId::Gemini => verify(rig::providers::gemini::Client::new(key)).await,
            ProviderId::Groq => verify(rig::providers::groq::Client::new(key)).await,
            ProviderId::OpenRouter => verify(rig::providers::openrouter::Client::new(key)).await,
            ProviderId::XAi => verify(rig::providers::xai::Client::new(key)).await,
            ProviderId::Mistral => verify(rig::providers::mistral::Client::new(key)).await,
            ProviderId::Cohere => verify(rig::providers::cohere::Client::new(key)).await,
            ProviderId::ChatGpt | ProviderId::Copilot | ProviderId::Ollama => Ok(VerificationStatus::Deferred),
        }
    }
}

async fn verify<C, E>(client: std::result::Result<C, E>) -> Result<VerificationStatus>
where
    C: VerifyClient,
{
    let client = client.map_err(|_| AppError::Auth("Failed to initialize credential verification".to_string()))?;
    client
        .verify()
        .await
        .map_err(|_| AppError::Auth("Credential verification failed; stored key was not changed".to_string()))?;
    Ok(VerificationStatus::Verified)
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::Credential;
    use std::collections::HashMap;

    fn request<'a>(model: &'a str, config_dir: &'a Path) -> ModelRequest<'a> {
        ModelRequest { model, config_dir }
    }

    fn auth_store() -> AuthStore {
        let credentials = ProviderId::API_KEY_PROVIDERS
            .into_iter()
            .filter(|provider| provider.credential_strategy() == CredentialStrategy::ApiKey)
            .map(|provider| {
                (
                    provider.as_str().to_string(),
                    Credential::ApiKey {
                        key: "fixture-key-not-secret".to_string(),
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        let mut auth_store = AuthStore::default();
        auth_store.credentials = credentials;
        auth_store
    }

    #[test]
    fn parses_provider_identities_and_only_documented_alias() {
        let cases = [
            ("anthropic", ProviderId::Anthropic),
            ("OPENAI", ProviderId::OpenAi),
            ("chatgpt", ProviderId::ChatGpt),
            ("copilot", ProviderId::Copilot),
            ("deepseek", ProviderId::DeepSeek),
            ("gemini", ProviderId::Gemini),
            ("google", ProviderId::Gemini),
            ("groq", ProviderId::Groq),
            ("ollama", ProviderId::Ollama),
            ("openrouter", ProviderId::OpenRouter),
            ("xai", ProviderId::XAi),
            ("mistral", ProviderId::Mistral),
            ("cohere", ProviderId::Cohere),
        ];
        for (input, expected) in cases {
            assert_eq!(ProviderId::from_str(input).unwrap(), expected);
        }
        assert!(ProviderId::from_str("open-ai").is_err());
    }

    #[test]
    fn keeps_openai_chatgpt_and_copilot_distinct() {
        assert_eq!(ProviderId::OpenAi.credential_strategy(), CredentialStrategy::ApiKey);
        assert_eq!(
            ProviderId::ChatGpt.credential_strategy(),
            CredentialStrategy::SubscriptionOAuth
        );
        assert_eq!(
            ProviderId::Copilot.credential_strategy(),
            CredentialStrategy::SubscriptionOAuth
        );
        assert_ne!(ProviderId::OpenAi, ProviderId::ChatGpt);
        assert_ne!(ProviderId::ChatGpt, ProviderId::Copilot);
    }

    #[test]
    fn constructs_every_provider_model_without_network_or_device_flow() {
        let auth = auth_store();
        let config_dir = std::env::temp_dir().join("rust-ai-provider-construction");
        for provider in ProviderId::ALL {
            let handle =
                ProviderFactory::create_model_for(provider, request("fixture-model", &config_dir), &auth).unwrap();
            assert_eq!(handle.label(), Some(provider.as_str()));
        }
    }

    #[test]
    fn unknown_provider_fails_before_auth_lookup() {
        let error =
            ProviderFactory::create_model("unknown", request("model", Path::new("unused")), &AuthStore::default())
                .unwrap_err();
        assert!(error.to_string().contains("unsupported AI provider"));
    }

    #[test]
    fn missing_key_names_variable_without_credentials() {
        let error = ProviderFactory::create_model_for(
            ProviderId::Anthropic,
            request("model", Path::new("unused")),
            &AuthStore::default(),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "Auth error: ANTHROPIC_API_KEY is not set");
    }

    #[test]
    fn subscription_models_use_their_own_rig_identity() {
        let auth = auth_store();
        let root = Path::new("oauth-fixture-root");
        let chatgpt = ProviderFactory::create_model_for(ProviderId::ChatGpt, request("model", root), &auth).unwrap();
        let copilot = ProviderFactory::create_model_for(ProviderId::Copilot, request("model", root), &auth).unwrap();
        assert_eq!(chatgpt.label(), Some("chatgpt"));
        assert_eq!(copilot.label(), Some("copilot"));
    }

    #[tokio::test]
    async fn unsupported_verification_is_explicit() {
        let status = RigCredentialVerifier
            .verify(ProviderId::Ollama, "unused")
            .await
            .unwrap();
        assert_eq!(status, VerificationStatus::Deferred);
    }

    #[test]
    fn curated_listing_is_labeled_as_not_live() {
        let catalog = curated(ProviderId::ChatGpt);
        assert!(catalog.source_label().contains("not live discovery"));
        assert!(catalog.models().contains(&"gpt-5.3-codex"));
    }

    #[test]
    fn supported_listing_capabilities_are_rig_backed() {
        fn lists<T: ModelListingClient>() {}
        lists::<rig::providers::anthropic::Client>();
        lists::<rig::providers::openai::Client>();
        lists::<rig::providers::deepseek::Client>();
        lists::<rig::providers::gemini::Client>();
        lists::<rig::providers::groq::Client>();
        lists::<rig::providers::ollama::Client>();
        lists::<rig::providers::openrouter::Client>();
        lists::<rig::providers::mistral::Client>();
        lists::<rig::providers::copilot::Client>();
    }

    #[test]
    fn model_debug_output_does_not_expose_key() {
        let sentinel = "credential-secret-sentinel";
        let mut auth = AuthStore::default();
        auth.credentials.insert(
            "openai".to_string(),
            Credential::ApiKey {
                key: sentinel.to_string(),
            },
        );
        let handle =
            ProviderFactory::create_model_for(ProviderId::OpenAi, request("model", Path::new("unused")), &auth)
                .unwrap();
        assert!(!format!("{handle:?}").contains(sentinel));
    }
}
