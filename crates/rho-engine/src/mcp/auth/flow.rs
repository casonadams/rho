use crate::auth::loopback::LoopbackServer;
use crate::auth::pkce::{PkceChallenge, generate_state};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use rho_harness_core::auth::{OAuthLoginCallbacks, StoredCredential};
use rho_harness_core::error::{AppError, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_MCP_CLIENT_ID: &str = "rho-mcp-client";

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

pub async fn execute_mcp_pkce_flow(
    server_name: &str,
    auth_endpoint: &str,
    token_endpoint: &str,
    scope: Option<&str>,
    client_id: Option<&str>,
    callbacks: &dyn OAuthLoginCallbacks,
) -> Result<StoredCredential> {
    let pkce = PkceChallenge::generate();
    let state = generate_state();
    let loopback = LoopbackServer::bind().await?;
    let redirect_uri = loopback.redirect_uri("/callback");
    let cid = client_id.unwrap_or(DEFAULT_MCP_CLIENT_ID);

    let enc_redirect = utf8_percent_encode(&redirect_uri, NON_ALPHANUMERIC);
    let mut auth_url = format!(
        "{auth_endpoint}?response_type=code&client_id={cid}&redirect_uri={enc_redirect}&code_challenge={}&code_challenge_method=S256&state={state}",
        pkce.challenge
    );
    if let Some(s) = scope {
        auth_url.push_str("&scope=");
        auth_url.push_str(&utf8_percent_encode(s, NON_ALPHANUMERIC).to_string());
    }

    callbacks
        .on_auth_url(
            &auth_url,
            Some(&format!(
                "Complete authorization in your browser for MCP server '{server_name}'"
            )),
        )
        .await?;

    callbacks.on_progress("Waiting for browser authentication...").await?;

    let params = loopback.wait_for_callback(Duration::from_secs(120)).await?;

    if let Some(err) = params.error {
        return Err(AppError::Auth(format!("OAuth failed: {err}")));
    }

    let code = params
        .code
        .ok_or_else(|| AppError::Auth("No authorization code received".to_string()))?;

    if params.state.as_deref() != Some(&state) {
        return Err(AppError::Auth("OAuth state mismatch".to_string()));
    }

    callbacks
        .on_progress("Exchanging authorization code for token...")
        .await?;

    let mut form = HashMap::new();
    form.insert("grant_type", "authorization_code");
    form.insert("code", &code);
    form.insert("redirect_uri", &redirect_uri);
    form.insert("client_id", cid);
    form.insert("code_verifier", &pkce.verifier);

    let client = crate::auth::http::http_client();
    let res = client
        .post(token_endpoint)
        .form(&form)
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Token exchange request failed: {e}")))?;

    if !res.status().is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(AppError::Auth(format!("Token exchange failed: {body}")));
    }

    let token_data: TokenResponse = res
        .json()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to parse token response: {e}")))?;

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let expires_at_ms = token_data.expires_in.map(|secs| now_ms + (secs as i64) * 1000);

    Ok(StoredCredential::oauth(
        token_data.access_token,
        token_data.refresh_token,
        expires_at_ms,
    ))
}
