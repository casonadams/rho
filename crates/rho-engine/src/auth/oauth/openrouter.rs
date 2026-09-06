use super::http_client;
use crate::auth::loopback::LoopbackServer;
use crate::auth::pkce::PkceChallenge;
use rho_harness_core::auth::{OAuthLoginCallbacks, StoredCredential};
use rho_harness_core::error::{AppError, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

pub(crate) const OPENROUTER_AUTH_URL: &str = "https://openrouter.ai/auth";
pub(crate) const OPENROUTER_KEYS_URL: &str = "https://openrouter.ai/api/v1/auth/keys";

#[derive(Debug, Deserialize)]
struct OpenRouterKeyResponse {
    key: String,
}

pub(crate) fn build_auth_url(redirect_uri: Option<&str>, code_challenge: &str) -> String {
    if let Some(uri) = redirect_uri {
        format!(
            "{OPENROUTER_AUTH_URL}?callback_url={uri}&code_challenge={code_challenge}&code_challenge_method=S256&key_label=rho"
        )
    } else {
        format!("{OPENROUTER_AUTH_URL}?code_challenge={code_challenge}&code_challenge_method=S256&key_label=rho")
    }
}

pub(crate) fn build_exchange_body<'a>(code: &'a str, code_verifier: &'a str) -> HashMap<&'static str, &'a str> {
    let mut body = HashMap::new();
    body.insert("code", code);
    body.insert("code_verifier", code_verifier);
    body.insert("code_challenge_method", "S256");
    body
}

pub(crate) fn parse_key_response(json_str: &str) -> Result<String> {
    let data: OpenRouterKeyResponse = serde_json::from_str(json_str)
        .map_err(|e| AppError::Auth(format!("Failed to parse OpenRouter key response: {e}")))?;
    if data.key.trim().is_empty() {
        return Err(AppError::Auth("OpenRouter returned an empty API key".to_string()));
    }
    Ok(data.key)
}

async fn acquire_openrouter_browser_code(
    server: LoopbackServer,
    challenge: &str,
    callbacks: &dyn OAuthLoginCallbacks,
) -> Result<String> {
    let redirect_uri = server.redirect_uri("/callback");
    let auth_url = build_auth_url(Some(&redirect_uri), challenge);

    callbacks
        .on_auth_url(&auth_url, Some("Sign in with OpenRouter"))
        .await?;
    callbacks
        .on_progress("Waiting for OpenRouter authorization in browser...")
        .await?;

    let callback_res = server.wait_for_callback(Duration::from_secs(120)).await?;
    if let Some(err) = callback_res.error {
        let desc = callback_res.error_description.unwrap_or_default();
        return Err(AppError::Auth(
            format!("OpenRouter authorization failed: {err} {desc}")
                .trim()
                .to_string(),
        ));
    }
    callback_res
        .code
        .ok_or_else(|| AppError::Auth("No authorization code received from OpenRouter callback".to_string()))
}

async fn acquire_openrouter_headless_code(challenge: &str, callbacks: &dyn OAuthLoginCallbacks) -> Result<String> {
    let auth_url = build_auth_url(None, challenge);
    callbacks
        .on_auth_url(&auth_url, Some("Sign in with OpenRouter (Headless mode)"))
        .await?;
    let input = callbacks
        .on_prompt("Enter the authorization code displayed on OpenRouter:", false)
        .await?;
    let code = input.trim().to_string();
    if code.is_empty() {
        return Err(AppError::Auth("Authorization code cannot be empty".to_string()));
    }
    Ok(code)
}

async fn exchange_openrouter_key(code: &str, verifier: &str) -> Result<String> {
    let client = http_client();
    let body = build_exchange_body(code, verifier);

    let res = client
        .post(OPENROUTER_KEYS_URL)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to exchange OpenRouter key: {e}")))?;

    if !res.status().is_success() {
        let status = res.status();
        let err_body = res.text().await.unwrap_or_default();
        return Err(AppError::Auth(format!(
            "OpenRouter key exchange failed ({status}): {err_body}"
        )));
    }

    let text = res
        .text()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to read OpenRouter key response: {e}")))?;
    parse_key_response(&text)
}

async fn acquire_openrouter_code(challenge: &str, callbacks: &dyn OAuthLoginCallbacks) -> Result<String> {
    match LoopbackServer::bind().await {
        Ok(server) => acquire_openrouter_browser_code(server, challenge, callbacks).await,
        Err(_) => acquire_openrouter_headless_code(challenge, callbacks).await,
    }
}

pub async fn perform_openrouter_pkce(callbacks: &dyn OAuthLoginCallbacks) -> Result<StoredCredential> {
    let pkce = PkceChallenge::generate();
    let code = acquire_openrouter_code(&pkce.challenge, callbacks).await?;
    callbacks
        .on_progress("Exchanging authorization code for OpenRouter API key...")
        .await?;

    let key = exchange_openrouter_key(&code, &pkce.verifier).await?;
    Ok(StoredCredential::api_key(key))
}
