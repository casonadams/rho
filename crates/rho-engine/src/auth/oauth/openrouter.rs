//! OpenRouter PKCE OAuth flow for API key exchange.

use super::http_client;
use crate::auth::loopback::LoopbackServer;
use crate::auth::pkce::PkceChallenge;
use rho_harness_core::auth::{OAuthLoginCallbacks, StoredCredential};
use rho_harness_core::error::{AppError, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

#[derive(Deserialize)]
struct OpenRouterKeyResponse {
    key: String,
}

pub async fn perform_openrouter_pkce(callbacks: &dyn OAuthLoginCallbacks) -> Result<StoredCredential> {
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

    let client = http_client()?;

    let mut body = HashMap::new();
    body.insert("code", code);
    body.insert("code_verifier", pkce.verifier);
    body.insert("code_challenge_method", "S256".to_string());

    let res = client
        .post("https://openrouter.ai/api/v1/auth/keys")
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to exchange OpenRouter key: {e}")))?;

    let key_data: OpenRouterKeyResponse = res
        .json()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to parse OpenRouter key response: {e}")))?;

    Ok(StoredCredential::api_key(key_data.key))
}
