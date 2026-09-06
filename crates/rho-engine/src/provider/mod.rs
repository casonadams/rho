pub mod capabilities;
pub mod discovery;
pub mod store;

pub use capabilities::supports_tool_result_images;
pub use store::ModelStore;

use crate::auth::AuthStore;
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

static SHARED_HTTP_CLIENT: std::sync::LazyLock<reqwest::Client> = std::sync::LazyLock::new(|| {
    crate::install_crypto_provider();
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("Failed to build shared HTTP client")
});

fn validate_custom_provider_url(name: &str, url: &str, allow_private: bool) -> Result<()> {
    rho_harness_core::net::validate_url(url, allow_private)
        .map(|_| ())
        .map_err(|e| match e {
            AppError::Tool(message) => AppError::Provider(format!("Provider '{name}': {message}")),
            other => other,
        })
}

fn build_local_ollama_model(model: &str) -> Result<ModelHandle> {
    let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let client = rig::providers::ollama::Client::builder()
        .http_client(SHARED_HTTP_CLIENT.clone())
        .api_key("")
        .base_url(&host)
        .build()
        .map_err(|e| AppError::Provider(format!("Failed to initialize Ollama client: {e}")))?;
    Ok(ModelHandle::named(
        ProviderId::Local.as_str(),
        client.completion_model(model),
    ))
}

fn resolve_provider_key(provider: ProviderId, auth_store: &AuthStore) -> Result<String> {
    auth_store.get_key_sync(provider.as_str())?.ok_or_else(|| {
        AppError::Auth(format!(
            "Missing API key for provider '{}'. Run 'rho login {}' or set {}.",
            provider.as_str(),
            provider.as_str(),
            provider.api_key_env().unwrap_or("API key")
        ))
    })
}

fn chatgpt_client_headers(account_id: Option<&str>) -> reqwest::header::HeaderMap {
    let mut default_headers = reqwest::header::HeaderMap::new();
    default_headers.insert(
        "OpenAI-Beta",
        reqwest::header::HeaderValue::from_static("responses=experimental"),
    );
    default_headers.insert("originator", reqwest::header::HeaderValue::from_static("codex"));
    default_headers.insert("User-Agent", reqwest::header::HeaderValue::from_static("Codex/0.22.4"));
    if let Some(acc_id) = account_id
        && let Ok(val) = reqwest::header::HeaderValue::from_str(acc_id)
    {
        default_headers.insert("chatgpt-account-id", val.clone());
        default_headers.insert("ChatGPT-Account-Id", val);
    }
    default_headers
}

fn build_chatgpt_model((model, key): (&str, String), auth_store: &AuthStore) -> Result<ModelHandle> {
    let account_id = match auth_store.get_credential("chatgpt") {
        Some(rho_harness_core::auth::StoredCredential::OAuth {
            account_id: Some(id), ..
        }) => Some(id.clone()),
        _ => crate::auth::oauth::extract_chatgpt_account_id(&key),
    };

    let http_client = reqwest::Client::builder()
        .no_proxy()
        .default_headers(chatgpt_client_headers(account_id.as_deref()))
        .build()
        .map_err(|e| AppError::Other(e.into()))?;

    let client = rig::providers::chatgpt::Client::builder()
        .http_client(http_client)
        .api_key(rig::providers::chatgpt::ChatGPTAuth::AccessToken {
            access_token: key,
            account_id,
        })
        .originator("codex")
        .build()
        .map_err(|e| AppError::Provider(format!("Failed to initialize ChatGPT Codex client: {e}")))?;
    Ok(ModelHandle::named(
        ProviderId::ChatGpt.as_str(),
        client.completion_model(model),
    ))
}

fn build_gemini_model((model, key): (&str, String)) -> Result<ModelHandle> {
    let http_client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .map_err(|e| AppError::Other(e.into()))?;

    let client = rig::providers::gemini::Client::builder()
        .http_client(http_client)
        .api_key(&key)
        .build()
        .map_err(|e| AppError::Provider(format!("Failed to initialize Gemini client: {e}")))?;
    Ok(ModelHandle::named(
        ProviderId::Gemini.as_str(),
        client.completion_model(model),
    ))
}

fn build_antigravity_model(request: &ModelRequest<'_>, auth_store: &AuthStore) -> ModelHandle {
    let project_id = match auth_store.get_credential("antigravity") {
        Some(rho_harness_core::auth::StoredCredential::OAuth {
            account_id: Some(id), ..
        }) => id.clone(),
        _ => crate::auth::antigravity::stable_project_id("antigravity-default"),
    };
    let store = request
        .shared_auth
        .clone()
        .unwrap_or_else(|| std::sync::Arc::new(tokio::sync::Mutex::new(auth_store.clone())));
    let client = crate::antigravity::AntigravityClient::with_auth_store(store, project_id, request.model)
        .with_effort(request.thinking_level);
    crate::antigravity::into_handle(client)
}

fn build_claude_code_model(request: &ModelRequest<'_>, auth_store: &AuthStore) -> ModelHandle {
    let store = request
        .shared_auth
        .clone()
        .unwrap_or_else(|| std::sync::Arc::new(tokio::sync::Mutex::new(auth_store.clone())));
    let client =
        crate::claude::ClaudeClient::with_auth_store(store, request.model).with_thinking_level(request.thinking_level);
    crate::claude::into_handle(client)
}

macro_rules! match_standard_provider {
    ($provider:expr, $model:expr, $key:expr, $($id:ident => $client:path),* $(,)?) => {
        match $provider {
            $(
                ProviderId::$id => {
                    let c = <$client>::new($key)
                        .map_err(|e| AppError::Provider(format!("Failed to initialize {} client: {e}", $provider.as_str())))?;
                    Ok(ModelHandle::named($provider.as_str(), c.completion_model($model)))
                }
            )*
            _ => Err(AppError::Provider(format!("Unsupported standard provider '{}'", $provider.as_str()))),
        }
    };
}

fn build_rig_named_client(provider: ProviderId, model: &str, key: String) -> Result<ModelHandle> {
    match_standard_provider!(
        provider, model, key,
        Anthropic => rig::providers::anthropic::Client,
        DeepSeek => rig::providers::deepseek::Client,
        Groq => rig::providers::groq::Client,
        OpenRouter => rig::providers::openrouter::Client,
        XAi => rig::providers::xai::Client,
        Mistral => rig::providers::mistral::Client,
        Cohere => rig::providers::cohere::Client,
    )
}

fn build_ollama_cloud_model(model: &str, key: String) -> Result<ModelHandle> {
    let c = rig::providers::openai::Client::builder()
        .http_client(SHARED_HTTP_CLIENT.clone())
        .api_key(key)
        .base_url("https://ollama.com/v1")
        .build()
        .map_err(|e| AppError::Provider(format!("Failed to initialize Ollama Cloud client: {e}")))?;
    Ok(ModelHandle::named(
        ProviderId::OllamaCloud.as_str(),
        c.completion_model(model),
    ))
}

fn build_standard_client_model(provider: ProviderId, model: &str, key: String) -> Result<ModelHandle> {
    match provider {
        ProviderId::OpenAi => {
            let c = rig::providers::openai::Client::builder()
                .http_client(SHARED_HTTP_CLIENT.clone())
                .api_key(key)
                .build()
                .map_err(|e| AppError::Provider(format!("Failed to initialize OpenAI client: {e}")))?;
            Ok(ModelHandle::named(provider.as_str(), c.completion_model(model)))
        }
        ProviderId::Copilot => {
            let c = rig::providers::copilot::Client::builder()
                .github_access_token(key)
                .build()
                .map_err(|e| AppError::Provider(format!("Failed to initialize Copilot client: {e}")))?;
            Ok(ModelHandle::named(provider.as_str(), c.completion_model(model)))
        }
        ProviderId::OllamaCloud => build_ollama_cloud_model(model, key),
        _ => build_rig_named_client(provider, model, key),
    }
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
            return build_chatgpt_model((model, key), auth_store);
        }
        if provider == ProviderId::Gemini {
            return build_gemini_model((model, key));
        }
        build_standard_client_model(provider, model, key)
    }
}
