use super::ModelRequest;
use crate::auth::AuthStore;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::provider::ProviderId;
use rig::agent::ModelHandle;
use rig::client::CompletionClient;

pub(super) static SHARED_HTTP_CLIENT: std::sync::LazyLock<reqwest::Client> = std::sync::LazyLock::new(|| {
    crate::install_crypto_provider();
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("Failed to build shared HTTP client")
});

pub(super) fn validate_custom_provider_url(name: &str, url: &str, allow_private: bool) -> Result<()> {
    rho_harness_core::net::validate_url(url, allow_private)
        .map(|_| ())
        .map_err(|e| match e {
            AppError::Tool(message) => AppError::Provider(format!("Provider '{name}': {message}")),
            other => other,
        })
}

pub(super) fn build_local_ollama_model(model: &str) -> Result<ModelHandle> {
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

pub(super) fn resolve_provider_key(provider: ProviderId, auth_store: &AuthStore) -> Result<String> {
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

pub(super) fn build_chatgpt_model(model: &str, key: String, auth_store: &AuthStore) -> Result<ModelHandle> {
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

pub(super) fn build_gemini_model(model: &str, key: String) -> Result<ModelHandle> {
    let client = rig::providers::gemini::Client::builder()
        .http_client(SHARED_HTTP_CLIENT.clone())
        .api_key(&key)
        .build()
        .map_err(|e| AppError::Provider(format!("Failed to initialize Gemini client: {e}")))?;
    Ok(ModelHandle::named(
        ProviderId::Gemini.as_str(),
        client.completion_model(model),
    ))
}

pub(super) fn build_antigravity_model(request: &ModelRequest<'_>, auth_store: &AuthStore) -> ModelHandle {
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

pub(super) fn build_claude_code_model(request: &ModelRequest<'_>, auth_store: &AuthStore) -> ModelHandle {
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

pub(super) fn build_standard_client_model(provider: ProviderId, model: &str, key: String) -> Result<ModelHandle> {
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
