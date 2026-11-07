//! Google OAuth token exchange and user info endpoints.

use crate::auth::token_request::post_form_json;
use crate::http::CLIENT;
use rho_harness_core::error::{AppError, Result};
use serde::Deserialize;
use std::time::Duration;

pub const GOOGLE_CLIENT_ID: &str = "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
pub const GOOGLE_CLIENT_SECRET: &str = "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";
pub const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const GOOGLE_USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v1/userinfo?alt=json";
pub const REDIRECT_PORT: u16 = 51121;
pub const REDIRECT_PATH: &str = "/oauth-callback";
pub const REDIRECT_URI_ENCODED: &str = "http%3A%2F%2Flocalhost%3A51121%2Foauth-callback";
pub const SCOPES: &str = "https://www.googleapis.com/auth/aicode\
%20https://www.googleapis.com/auth/cloud-platform\
%20https://www.googleapis.com/auth/userinfo.email\
%20https://www.googleapis.com/auth/userinfo.profile\
%20https://www.googleapis.com/auth/cclog\
%20https://www.googleapis.com/auth/experimentsandconfigs";
pub const CALLBACK_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Deserialize)]
pub(super) struct GoogleTokenResponse {
    pub(super) access_token: String,
    #[serde(default)]
    pub(super) refresh_token: Option<String>,
    pub(super) expires_in: Option<i64>,
}

async fn parse_google_token_response(res: reqwest::Response) -> Result<GoogleTokenResponse> {
    if !res.status().is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(AppError::Auth(format!("Google token exchange failed: {body}")));
    }
    let token: GoogleTokenResponse = res
        .json()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to parse Google token response: {e}")))?;
    if token.refresh_token.is_none() {
        return Err(AppError::Auth(
            "No refresh token received. Re-run 'rho login antigravity' and allow offline access.".to_string(),
        ));
    }
    Ok(token)
}

pub(super) async fn exchange_code(token_url: &str, code: &str, verifier: &str) -> Result<GoogleTokenResponse> {
    let client = &CLIENT;
    let redirect_uri = format!("http://localhost:{REDIRECT_PORT}{REDIRECT_PATH}");
    let form = [
        ("client_id", GOOGLE_CLIENT_ID),
        ("client_secret", GOOGLE_CLIENT_SECRET),
        ("code", code),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri.as_str()),
        ("code_verifier", verifier),
    ];
    let res = client
        .post(token_url)
        .timeout(Duration::from_secs(15))
        .form(&form)
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to exchange Google OAuth token: {e}")))?;
    parse_google_token_response(res).await
}

pub(super) async fn fetch_user_email(userinfo_url: &str, access_token: &str) -> Option<String> {
    let client = &CLIENT;
    let res = client
        .get(userinfo_url)
        .timeout(Duration::from_secs(15))
        .bearer_auth(access_token)
        .send()
        .await
        .ok()?;
    if !res.status().is_success() {
        return None;
    }
    let value: serde_json::Value = res.json().await.ok()?;
    value.get("email").and_then(|v| v.as_str()).map(String::from)
}

pub(super) async fn refresh_google_token(token_url: &str, refresh: &str) -> Result<GoogleTokenResponse> {
    let form = [
        ("client_id", GOOGLE_CLIENT_ID),
        ("client_secret", GOOGLE_CLIENT_SECRET),
        ("refresh_token", refresh),
        ("grant_type", "refresh_token"),
    ];
    post_form_json(
        token_url,
        &form,
        &[],
        Duration::from_secs(15),
        "Antigravity token refresh",
    )
    .await
}
