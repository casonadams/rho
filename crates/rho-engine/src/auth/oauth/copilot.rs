//! GitHub Copilot device-flow OAuth and Copilot internal token refresh.

use super::http_client;
use rho_harness_core::auth::{DeviceCodeInfo, OAuthLoginCallbacks, StoredCredential};
use rho_harness_core::error::{AppError, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

const COPILOT_CLIENT_ID: &str = "Iv1.b507a08c87ecfe81";
const COPILOT_DEVICE_URL: &str = "https://github.com/login/device/code";
const COPILOT_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const COPILOT_INTERNAL_URL: &str = "https://api.github.com/copilot_internal/v2/token";

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

async fn request_device_code(client: &reqwest::Client) -> Result<DeviceCodeResponse> {
    let mut form = HashMap::new();
    form.insert("client_id", COPILOT_CLIENT_ID);
    form.insert("scope", "read:user");

    let res = client
        .post(COPILOT_DEVICE_URL)
        .header("Accept", "application/json")
        .form(&form)
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Device code request failed: {e}")))?;

    res.json()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to parse device code response: {e}")))
}

pub(crate) fn parse_poll_payload(json: &serde_json::Value) -> Option<Result<String>> {
    if let Some(token) = json.get("access_token").and_then(|t| t.as_str()) {
        Some(Ok(token.to_string()))
    } else {
        match json.get("error").and_then(|e| e.as_str()) {
            Some("authorization_pending") | None => None,
            Some(err) => Some(Err(AppError::Auth(format!("Device code failed: {err}")))),
        }
    }
}

pub(crate) async fn check_poll_response(resp: reqwest::Response) -> Option<Result<String>> {
    if !resp.status().is_success() {
        return None;
    }
    let json = resp.json::<serde_json::Value>().await.ok()?;
    parse_poll_payload(&json)
}

pub(crate) fn build_device_token_form(device_code: &str) -> [(&'static str, &str); 3] {
    [
        ("client_id", COPILOT_CLIENT_ID),
        ("device_code", device_code),
        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
    ]
}

async fn poll_device_token(
    client: &reqwest::Client,
    device_code: &str,
    interval_secs: u64,
    expires_in: u64,
) -> Result<String> {
    let interval = Duration::from_secs(interval_secs.max(5));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(expires_in);

    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(AppError::Auth("Device code login timed out".to_string()));
        }
        tokio::time::sleep(interval).await;
        let form = build_device_token_form(device_code);
        if let Ok(resp) = client
            .post(COPILOT_TOKEN_URL)
            .header("Accept", "application/json")
            .form(&form)
            .send()
            .await
            && let Some(result) = check_poll_response(resp).await
        {
            return result;
        }
    }
}

async fn retrieve_copilot_token(client: &reqwest::Client, github_token: &str) -> Result<CopilotInternalToken> {
    let copilot_res = client
        .get(COPILOT_INTERNAL_URL)
        .header("Authorization", format!("token {github_token}"))
        .header("Accept", "application/json")
        .header("User-Agent", "GitHubCopilotChat/0.22.4")
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to retrieve Copilot token: {e}")))?;

    copilot_res
        .json()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to parse Copilot token: {e}")))
}

pub(crate) fn build_copilot_credential(token: String, github_token: String, expires_at_sec: i64) -> StoredCredential {
    StoredCredential::oauth(token, Some(github_token), Some(expires_at_sec * 1000))
}

pub async fn perform_copilot_device_flow(callbacks: &dyn OAuthLoginCallbacks) -> Result<StoredCredential> {
    let client = http_client();
    let device_info = request_device_code(client).await?;
    let info = DeviceCodeInfo {
        user_code: &device_info.user_code,
        verification_uri: &device_info.verification_uri,
        interval_secs: device_info.interval,
        expires_in_secs: device_info.expires_in,
    };
    callbacks.on_device_code(&info).await?;

    let github_token = poll_device_token(
        client,
        &device_info.device_code,
        device_info.interval,
        device_info.expires_in,
    )
    .await?;
    let copilot_data = retrieve_copilot_token(client, &github_token).await?;

    Ok(build_copilot_credential(
        copilot_data.token,
        github_token,
        copilot_data.expires_at,
    ))
}

pub async fn refresh_copilot_token(github_token: &str) -> Result<StoredCredential> {
    let client = http_client();
    let copilot_data = retrieve_copilot_token(client, github_token).await?;
    Ok(build_copilot_credential(
        copilot_data.token,
        github_token.to_string(),
        copilot_data.expires_at,
    ))
}
