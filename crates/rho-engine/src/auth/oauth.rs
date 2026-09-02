//! OAuth 2.0 PKCE and Device Code login and refresh handlers.

use super::loopback::LoopbackServer;
use super::pkce::{PkceChallenge, generate_state};
use rho_harness_core::auth::{DeviceCodeInfo, OAuthLoginCallbacks, StoredCredential};
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::provider::ProviderId;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

const OPENAI_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_AUTH_URL: &str = "https://auth.openai.com/oauth/authorize";
const OPENAI_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const OPENAI_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const OPENAI_SCOPE: &str = "openid profile email offline_access";

const COPILOT_CLIENT_ID: &str = "Iv1.b507a08c87ecfe81";
const COPILOT_DEVICE_URL: &str = "https://github.com/login/device/code";
const COPILOT_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const COPILOT_INTERNAL_URL: &str = "https://api.github.com/copilot_internal/v2/token";

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

#[derive(Debug, Deserialize)]
struct CopilotInternalToken {
    token: String,
    expires_at: i64,
}

pub async fn perform_oauth_login(
    provider: ProviderId,
    callbacks: &dyn OAuthLoginCallbacks,
) -> Result<StoredCredential> {
    match provider {
        ProviderId::ChatGpt => perform_openai_pkce(callbacks).await,
        ProviderId::Copilot => perform_copilot_device_flow(callbacks).await,
        ProviderId::OpenRouter => perform_openrouter_pkce(callbacks).await,
        _ => Err(AppError::Auth(format!(
            "OAuth login is not supported for provider '{provider}'"
        ))),
    }
}

pub async fn refresh_oauth_token(provider: ProviderId, credential: &StoredCredential) -> Result<StoredCredential> {
    match credential {
        StoredCredential::ApiKey { .. } => Ok(credential.clone()),
        StoredCredential::OAuth {
            refresh_token: Some(refresh),
            ..
        } => match provider {
            ProviderId::ChatGpt => refresh_openai_token(refresh).await,
            ProviderId::Copilot => refresh_copilot_token(refresh).await,
            _ => Ok(credential.clone()),
        },
        StoredCredential::OAuth { .. } => Err(AppError::Auth(format!(
            "OAuth token for '{provider}' has expired and has no refresh token. Please re-run /login."
        ))),
    }
}

async fn perform_openai_pkce(callbacks: &dyn OAuthLoginCallbacks) -> Result<StoredCredential> {
    let pkce = PkceChallenge::generate();
    let state = generate_state();

    let server = match LoopbackServer::bind_port(1455).await {
        Ok(s) => s,
        Err(e) => {
            return Err(AppError::Auth(format!(
                "Failed to start OAuth callback listener on port 1455: {e}.\n\
                 Ensure no other process is using port 1455 and try again."
            )));
        }
    };

    let auth_url = format!(
        "{OPENAI_AUTH_URL}?response_type=code&client_id={OPENAI_CLIENT_ID}&redirect_uri={OPENAI_REDIRECT_URI}\
         &scope={OPENAI_SCOPE}&code_challenge={}&code_challenge_method=S256&state={state}\
         &id_token_add_organizations=true&codex_cli_simplified_flow=true&originator=rho",
        pkce.challenge
    );

    callbacks
        .on_auth_url(&auth_url, Some("A browser window will open. Complete login to finish."))
        .await?;
    callbacks.on_progress("Waiting for browser authorization...").await?;

    let callback_res = server.wait_for_callback(Duration::from_secs(120)).await?;

    if let Some(err) = callback_res.error {
        let desc = callback_res.error_description.unwrap_or_default();
        return Err(AppError::Auth(format!("OAuth failed: {err} {desc}")));
    }

    let code = callback_res
        .code
        .ok_or_else(|| AppError::Auth("No authorization code received from callback".to_string()))?;

    callbacks
        .on_progress("Exchanging authorization code for tokens...")
        .await?;

    let http_client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .map_err(|e| AppError::Other(e.into()))?;

    let mut form = HashMap::new();
    form.insert("grant_type", "authorization_code");
    form.insert("client_id", OPENAI_CLIENT_ID);
    form.insert("code", &code);
    form.insert("code_verifier", &pkce.verifier);
    form.insert("redirect_uri", OPENAI_REDIRECT_URI);

    let res = http_client
        .post(OPENAI_TOKEN_URL)
        .form(&form)
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to exchange OAuth token: {e}")))?;

    if !res.status().is_success() {
        let err_body = res.text().await.unwrap_or_default();
        return Err(AppError::Auth(format!("Token exchange failed: {err_body}")));
    }

    let token_data: TokenResponse = res
        .json()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to parse token response: {e}")))?;

    let expires_at_ms = token_data
        .expires_in
        .map(|sec| chrono::Utc::now().timestamp_millis() + sec * 1000);

    let account_id = extract_chatgpt_account_id(&token_data.access_token);

    let mut cred = StoredCredential::oauth(token_data.access_token, token_data.refresh_token, expires_at_ms);
    if let StoredCredential::OAuth {
        account_id: ref mut id_field,
        ..
    } = cred
    {
        *id_field = account_id;
    }

    Ok(cred)
}

pub fn extract_chatgpt_account_id(jwt: &str) -> Option<String> {
    use base64::Engine;
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};

    let payload_b64 = jwt.split('.').nth(1)?;
    let mut padded = payload_b64.to_string();
    let rem = padded.len() % 4;
    if rem > 0 {
        padded.push_str(&"=".repeat(4 - rem));
    }

    let decoded = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .or_else(|_| URL_SAFE.decode(&padded))
        .or_else(|_| STANDARD_NO_PAD.decode(payload_b64))
        .or_else(|_| STANDARD.decode(&padded))
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    value
        .get("https://api.openai.com/auth")
        .and_then(|v| v.get("chatgpt_account_id"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

async fn refresh_openai_token(refresh_token: &str) -> Result<StoredCredential> {
    let http_client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .map_err(|e| AppError::Other(e.into()))?;

    let mut form = HashMap::new();
    form.insert("grant_type", "refresh_token");
    form.insert("client_id", OPENAI_CLIENT_ID);
    form.insert("refresh_token", refresh_token);

    let res = http_client
        .post(OPENAI_TOKEN_URL)
        .form(&form)
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Token refresh request failed: {e}")))?;

    if !res.status().is_success() {
        let err = res.text().await.unwrap_or_default();
        return Err(AppError::Auth(format!("OAuth token refresh failed: {err}")));
    }

    let token_data: TokenResponse = res
        .json()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to parse refresh token response: {e}")))?;

    let expires_at_ms = token_data
        .expires_in
        .map(|sec| chrono::Utc::now().timestamp_millis() + sec * 1000);

    let account_id = extract_chatgpt_account_id(&token_data.access_token);

    let mut cred = StoredCredential::oauth(
        token_data.access_token,
        token_data.refresh_token.or_else(|| Some(refresh_token.to_string())),
        expires_at_ms,
    );
    if let StoredCredential::OAuth {
        account_id: ref mut id_field,
        ..
    } = cred
    {
        *id_field = account_id;
    }

    Ok(cred)
}

async fn perform_copilot_device_flow(callbacks: &dyn OAuthLoginCallbacks) -> Result<StoredCredential> {
    let http_client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .map_err(|e| AppError::Other(e.into()))?;

    let mut form = HashMap::new();
    form.insert("client_id", COPILOT_CLIENT_ID);
    form.insert("scope", "read:user");

    let res = http_client
        .post(COPILOT_DEVICE_URL)
        .header("Accept", "application/json")
        .form(&form)
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Device code request failed: {e}")))?;

    let device_info: DeviceCodeResponse = res
        .json()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to parse device code response: {e}")))?;

    let info = DeviceCodeInfo {
        user_code: &device_info.user_code,
        verification_uri: &device_info.verification_uri,
        interval_secs: device_info.interval,
        expires_in_secs: device_info.expires_in,
    };
    callbacks.on_device_code(&info).await?;

    let interval = Duration::from_secs(device_info.interval.max(5));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(device_info.expires_in);

    let github_token = loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(AppError::Auth("Device code login timed out".to_string()));
        }
        tokio::time::sleep(interval).await;

        let mut poll_form = HashMap::new();
        poll_form.insert("client_id", COPILOT_CLIENT_ID);
        poll_form.insert("device_code", &device_info.device_code);
        poll_form.insert("grant_type", "urn:ietf:params:oauth:grant-type:device_code");

        let poll_res = http_client
            .post(COPILOT_TOKEN_URL)
            .header("Accept", "application/json")
            .form(&poll_form)
            .send()
            .await;

        if let Ok(resp) = poll_res
            && resp.status().is_success()
            && let Ok(json) = resp.json::<serde_json::Value>().await
        {
            if let Some(token) = json.get("access_token").and_then(|t| t.as_str()) {
                break token.to_string();
            }
            if let Some(err) = json.get("error").and_then(|e| e.as_str()) {
                if err == "authorization_pending" {
                    continue;
                }
                return Err(AppError::Auth(format!("Device code failed: {err}")));
            }
        }
    };

    let copilot_res = http_client
        .get(COPILOT_INTERNAL_URL)
        .header("Authorization", format!("token {github_token}"))
        .header("Accept", "application/json")
        .header("User-Agent", "GitHubCopilotChat/0.22.4")
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to retrieve Copilot token: {e}")))?;

    let copilot_data: CopilotInternalToken = copilot_res
        .json()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to parse Copilot token: {e}")))?;

    Ok(StoredCredential::oauth(
        copilot_data.token,
        Some(github_token),
        Some(copilot_data.expires_at * 1000),
    ))
}

async fn refresh_copilot_token(github_token: &str) -> Result<StoredCredential> {
    let http_client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .map_err(|e| AppError::Other(e.into()))?;

    let copilot_res = http_client
        .get(COPILOT_INTERNAL_URL)
        .header("Authorization", format!("token {github_token}"))
        .header("Accept", "application/json")
        .header("User-Agent", "GitHubCopilotChat/0.22.4")
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to refresh Copilot token: {e}")))?;

    let copilot_data: CopilotInternalToken = copilot_res
        .json()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to parse refreshed Copilot token: {e}")))?;

    Ok(StoredCredential::oauth(
        copilot_data.token,
        Some(github_token.to_string()),
        Some(copilot_data.expires_at * 1000),
    ))
}

async fn perform_openrouter_pkce(callbacks: &dyn OAuthLoginCallbacks) -> Result<StoredCredential> {
    let pkce = PkceChallenge::generate();
    let server = LoopbackServer::bind().await?;
    let redirect_uri = server.redirect_uri("/callback");

    let auth_url = format!(
        "https://openrouter.ai/auth?callback_url={redirect_uri}&code_challenge={}&code_challenge_method=S256",
        pkce.challenge
    );

    callbacks
        .on_auth_url(&auth_url, Some("Sign in with OpenRouter"))
        .await?;
    callbacks.on_progress("Waiting for OpenRouter authorization...").await?;

    let callback_res = server.wait_for_callback(Duration::from_secs(120)).await?;

    let code = callback_res
        .code
        .ok_or_else(|| AppError::Auth("No code received from OpenRouter callback".to_string()))?;

    let http_client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .map_err(|e| AppError::Other(e.into()))?;

    let mut body = HashMap::new();
    body.insert("code", code);
    body.insert("code_verifier", pkce.verifier);
    body.insert("code_challenge_method", "S256".to_string());

    let res = http_client
        .post("https://openrouter.ai/api/v1/auth/keys")
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to exchange OpenRouter key: {e}")))?;

    #[derive(Deserialize)]
    struct OpenRouterKeyResponse {
        key: String,
    }

    let key_data: OpenRouterKeyResponse = res
        .json()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to parse OpenRouter key response: {e}")))?;

    Ok(StoredCredential::api_key(key_data.key))
}
