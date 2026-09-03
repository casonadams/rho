//! Google OAuth for the Antigravity (Cloud Code Assist) provider.
//!
//! Mirrors the flow pi-antigravity uses: Google's public Antigravity desktop
//! OAuth client, PKCE S256, fixed loopback redirect on port 51121, then Cloud
//! Code Assist project discovery over the `v1internal` endpoints.

use super::loopback::LoopbackServer;
use super::pkce::{PkceChallenge, generate_state};
use crate::antigravity::load_project_id;
use rho_harness_core::auth::{OAuthLoginCallbacks, StoredCredential};
use rho_harness_core::error::{AppError, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::Duration;

const GOOGLE_CLIENT_ID: &str = "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
const GOOGLE_CLIENT_SECRET: &str = "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";
const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v1/userinfo?alt=json";
const REDIRECT_PORT: u16 = 51121;
const REDIRECT_PATH: &str = "/oauth-callback";
const REDIRECT_URI_ENCODED: &str = "http%3A%2F%2Flocalhost%3A51121%2Foauth-callback";
const SCOPES: &str = "https://www.googleapis.com/auth/aicode\
%20https://www.googleapis.com/auth/cloud-platform\
%20https://www.googleapis.com/auth/userinfo.email\
%20https://www.googleapis.com/auth/userinfo.profile\
%20https://www.googleapis.com/auth/cclog\
%20https://www.googleapis.com/auth/experimentsandconfigs";
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

/// Stable UUID-shaped fallback project id derived from a seed (account email).
pub fn stable_project_id(seed: &str) -> String {
    let digest = Sha256::digest(format!("antigravity:{seed}").as_bytes());
    let hex: String = digest.iter().take(16).map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .map_err(|e| AppError::Other(e.into()))
}

async fn exchange_code(code: &str, verifier: &str) -> Result<GoogleTokenResponse> {
    let client = http_client()?;
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
        .post(GOOGLE_TOKEN_URL)
        .form(&form)
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to exchange Google OAuth token: {e}")))?;
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

async fn fetch_user_email(access_token: &str) -> Option<String> {
    let client = http_client().ok()?;
    let res = client
        .get(GOOGLE_USERINFO_URL)
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

pub async fn perform_login(callbacks: &dyn OAuthLoginCallbacks) -> Result<StoredCredential> {
    let pkce = PkceChallenge::generate();
    let state = generate_state();

    let server = LoopbackServer::bind_port(REDIRECT_PORT).await.map_err(|e| {
        AppError::Auth(format!(
            "Failed to bind OAuth callback listener on port {REDIRECT_PORT}: {e}.\n\
             Close the process using port {REDIRECT_PORT} and try again."
        ))
    })?;

    let auth_url = format!(
        "{GOOGLE_AUTH_URL}?response_type=code&client_id={GOOGLE_CLIENT_ID}&redirect_uri={REDIRECT_URI_ENCODED}\
         &scope={SCOPES}&code_challenge={}&code_challenge_method=S256&state={state}\
         &access_type=offline&prompt=consent",
        pkce.challenge
    );

    callbacks
        .on_auth_url(&auth_url, Some("Complete Google sign-in to finish."))
        .await?;
    callbacks.on_progress("Waiting for Google authorization...").await?;

    let callback = server.wait_for_callback(CALLBACK_TIMEOUT).await?;
    if let Some(err) = callback.error {
        let desc = callback.error_description.unwrap_or_default();
        return Err(AppError::Auth(format!("OAuth failed: {err} {desc}")));
    }
    let code = callback
        .code
        .ok_or_else(|| AppError::Auth("No authorization code received from callback".to_string()))?;
    if callback.state.as_deref() != Some(state.as_str()) {
        return Err(AppError::Auth("OAuth state mismatch".to_string()));
    }

    callbacks
        .on_progress("Exchanging authorization code for tokens...")
        .await?;
    let token = exchange_code(&code, &pkce.verifier).await?;

    let expires_at_ms = token
        .expires_in
        .map(|sec| chrono::Utc::now().timestamp_millis() + sec * 1000 - 5 * 60 * 1000);

    let email = fetch_user_email(&token.access_token).await;
    let project_id = load_project_id(&token.access_token)
        .await
        .unwrap_or_else(|| stable_project_id(email.as_deref().unwrap_or("antigravity-default")));

    let mut cred = StoredCredential::oauth(token.access_token, token.refresh_token, expires_at_ms);
    if let StoredCredential::OAuth {
        account_id,
        account_email,
        ..
    } = &mut cred
    {
        *account_id = Some(project_id);
        *account_email = email;
    }
    Ok(cred)
}

pub async fn refresh_credential(credential: &StoredCredential) -> Result<StoredCredential> {
    let StoredCredential::OAuth {
        refresh_token: Some(refresh),
        account_id,
        account_email,
        ..
    } = credential
    else {
        return Err(AppError::Auth(
            "Antigravity token has expired and has no refresh token. Re-run 'rho login antigravity'.".to_string(),
        ));
    };

    let client = http_client()?;
    let form = [
        ("client_id", GOOGLE_CLIENT_ID),
        ("client_secret", GOOGLE_CLIENT_SECRET),
        ("refresh_token", refresh.as_str()),
        ("grant_type", "refresh_token"),
    ];
    let res = client
        .post(GOOGLE_TOKEN_URL)
        .form(&form)
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Antigravity token refresh failed: {e}")))?;
    if !res.status().is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(AppError::Auth(format!("Antigravity token refresh failed: {body}")));
    }
    let token: GoogleTokenResponse = res
        .json()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to parse refresh response: {e}")))?;

    let expires_at_ms = token
        .expires_in
        .map(|sec| chrono::Utc::now().timestamp_millis() + sec * 1000 - 5 * 60 * 1000);

    // Google does not rotate the refresh token on this grant; keep the stored one.
    let mut cred = StoredCredential::oauth(
        token.access_token,
        token.refresh_token.or_else(|| Some(refresh.clone())),
        expires_at_ms,
    );
    if let StoredCredential::OAuth {
        account_id: stored_id,
        account_email: stored_email,
        ..
    } = &mut cred
    {
        *stored_id = account_id.clone();
        *stored_email = account_email.clone();
    }
    Ok(cred)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_project_id_is_uuid_shaped_and_deterministic() {
        let a = stable_project_id("user@example.com");
        let b = stable_project_id("user@example.com");
        let c = stable_project_id("other@example.com");
        assert_eq!(a, b);
        assert_ne!(a, c);
        let parts: Vec<_> = a.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0].len(), 8);
        assert_eq!(parts[1].len(), 4);
    }
}
