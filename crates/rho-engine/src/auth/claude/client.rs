use rho_harness_core::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use std::time::Duration;

pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
pub const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
pub const PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";
pub const REDIRECT_URI: &str = "https://platform.claude.com/oauth/code/callback";
pub const SCOPES: &str = "user:inference user:profile user:sessions:claude_code user:mcp_servers";
pub const TOKEN_TIMEOUT: Duration = Duration::from_secs(60);
pub const USER_AGENT: &str = "claude-cli/2.1.62";
pub const ANTHROPIC_BETA: &str = "claude-code-20250219,oauth-2025-04-20";

static CLAUDE_AUTH_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(TOKEN_TIMEOUT)
        .build()
        .unwrap_or_default()
});

fn claude_auth_client() -> &'static reqwest::Client {
    &CLAUDE_AUTH_CLIENT
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClaudeTokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub organization: Option<ClaudeProfileOrganization>,
    #[serde(default)]
    pub account: Option<ClaudeProfileAccount>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClaudeProfileResponse {
    #[serde(default)]
    pub account: Option<ClaudeProfileAccount>,
    #[serde(default)]
    pub organization: Option<ClaudeProfileOrganization>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClaudeProfileAccount {
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub email_address: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClaudeProfileOrganization {
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

pub async fn exchange_code(code: &str, verifier: &str) -> Result<ClaudeTokenResponse> {
    exchange_code_with_redirect(code, verifier, REDIRECT_URI).await
}

pub async fn exchange_code_with_redirect(
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<ClaudeTokenResponse> {
    let clean_code = code.split_once('#').map(|(c, _)| c).unwrap_or(code).trim();
    let form = [
        ("grant_type", "authorization_code"),
        ("client_id", CLIENT_ID),
        ("code", clean_code),
        ("code_verifier", verifier),
        ("redirect_uri", redirect_uri),
    ];
    let res = claude_auth_client()
        .post(TOKEN_URL)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .form(&form)
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Claude token exchange network error: {e}")))?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(AppError::Auth(format!(
            "Claude token exchange failed ({status}): {body}"
        )));
    }

    res.json::<ClaudeTokenResponse>()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to parse Claude token response: {e}")))
}

pub async fn refresh_token(refresh: &str) -> Result<ClaudeTokenResponse> {
    let form = [
        ("grant_type", "refresh_token"),
        ("client_id", CLIENT_ID),
        ("refresh_token", refresh),
    ];
    let res = claude_auth_client()
        .post(TOKEN_URL)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .form(&form)
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Claude token refresh network error: {e}")))?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(AppError::Auth(format!(
            "Claude token refresh failed ({status}): {body}"
        )));
    }

    res.json::<ClaudeTokenResponse>()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to parse Claude refresh response: {e}")))
}

pub async fn fetch_profile(access_token: &str) -> Option<ClaudeProfileResponse> {
    let res = claude_auth_client()
        .get(PROFILE_URL)
        .bearer_auth(access_token)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header("anthropic-beta", ANTHROPIC_BETA)
        .send()
        .await
        .ok()?;

    if !res.status().is_success() {
        return None;
    }

    res.json::<ClaudeProfileResponse>().await.ok()
}
