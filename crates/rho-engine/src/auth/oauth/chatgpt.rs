//! OpenAI / ChatGPT PKCE OAuth login and token refresh flow.

use super::http_client;
use super::jwt::extract_chatgpt_account_id;
use crate::auth::loopback::LoopbackServer;
use crate::auth::pkce::{PkceChallenge, generate_state};
use rho_harness_core::auth::{OAuthLoginCallbacks, StoredCredential};
use rho_harness_core::error::{AppError, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

const OPENAI_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_AUTH_URL: &str = "https://auth.openai.com/oauth/authorize";
const OPENAI_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const OPENAI_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const OPENAI_SCOPE: &str = "openid profile email offline_access";

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

pub async fn perform_openai_pkce(callbacks: &dyn OAuthLoginCallbacks) -> Result<StoredCredential> {
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

    let client = http_client();
    let mut form = HashMap::new();
    form.insert("grant_type", "authorization_code");
    form.insert("client_id", OPENAI_CLIENT_ID);
    form.insert("code", &code);
    form.insert("code_verifier", &pkce.verifier);
    form.insert("redirect_uri", OPENAI_REDIRECT_URI);

    let res = client
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

pub async fn refresh_openai_token(refresh_token: &str) -> Result<StoredCredential> {
    let client = http_client();

    let mut form = HashMap::new();
    form.insert("grant_type", "refresh_token");
    form.insert("client_id", OPENAI_CLIENT_ID);
    form.insert("refresh_token", refresh_token);

    let res = client
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
