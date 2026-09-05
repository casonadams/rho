use super::client::{AUTHORIZE_URL, CLIENT_ID, REDIRECT_URI, SCOPES};
use crate::auth::loopback::LoopbackServer;
use rho_harness_core::auth::OAuthLoginCallbacks;
use rho_harness_core::error::{AppError, Result};
use std::time::Duration;

pub const DEFAULT_LOOPBACK_PORT: u16 = 51122;
pub const CALLBACK_TIMEOUT: Duration = Duration::from_secs(5 * 60);

pub fn build_authorize_url(redirect_uri: &str, challenge: &str, state: &str) -> String {
    format!(
        "{AUTHORIZE_URL}?response_type=code&client_id={CLIENT_ID}&redirect_uri={redirect_uri}\
         &scope={SCOPES}&code_challenge={challenge}&code_challenge_method=S256&state={state}&code=true"
    )
}

pub fn parse_auth_code_and_state(input: &str) -> (String, Option<String>) {
    let trimmed = input.trim();
    if let Some((_, query)) = trimmed.split_once('?') {
        let mut code = None;
        let mut state = None;
        for pair in query.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                match k {
                    "code" => code = Some(v.to_string()),
                    "state" => state = Some(v.to_string()),
                    _ => {}
                }
            }
        }
        if let Some(c) = code {
            return (c, state);
        }
    }
    if let Some((c, s)) = trimmed.split_once('#') {
        return (c.trim().to_string(), Some(s.trim().to_string()));
    }
    (trimmed.to_string(), None)
}

fn is_headless() -> bool {
    std::env::var("SSH_CLIENT").is_ok()
        || std::env::var("SSH_TTY").is_ok()
        || std::env::var("SSH_CONNECTION").is_ok()
        || std::env::var("RHO_FORCE_HEADLESS").is_ok()
        || (std::env::var("DISPLAY").is_err() && std::env::var("WAYLAND_DISPLAY").is_err() && cfg!(target_os = "linux"))
}

async fn bind_listener() -> Result<LoopbackServer> {
    match LoopbackServer::bind_port(DEFAULT_LOOPBACK_PORT).await {
        Ok(server) => Ok(server),
        Err(_) => LoopbackServer::bind().await,
    }
}

pub async fn acquire_auth_code(
    callbacks: &dyn OAuthLoginCallbacks,
    challenge: &str,
    state: &str,
) -> Result<(String, String)> {
    if !is_headless()
        && let Ok(server) = bind_listener().await
    {
        let redirect_uri = server.redirect_uri("/callback");
        let auth_url = build_authorize_url(&redirect_uri, challenge, state);
        callbacks
            .on_auth_url(&auth_url, Some("Complete Claude sign-in in your browser to finish."))
            .await?;
        callbacks.on_progress("Waiting for browser authorization...").await?;

        let callback = server.wait_for_callback(CALLBACK_TIMEOUT).await?;
        if let Some(err) = callback.error {
            let desc = callback.error_description.unwrap_or_default();
            return Err(AppError::Auth(format!("OAuth failed: {err} {desc}")));
        }
        let code = callback
            .code
            .ok_or_else(|| AppError::Auth("No authorization code received from callback".to_string()))?;
        if callback.state.as_deref() != Some(state) {
            return Err(AppError::Auth("OAuth state mismatch".to_string()));
        }
        return Ok((code, redirect_uri));
    }

    let auth_url = build_authorize_url(REDIRECT_URI, challenge, state);
    callbacks
        .on_auth_url(
            &auth_url,
            Some("Complete Claude sign-in in your browser, then copy the authorization code:"),
        )
        .await?;
    let input = callbacks
        .on_prompt("Paste authorization code (or CODE#STATE):", false)
        .await?;
    let (code, pasted_state) = parse_auth_code_and_state(&input);
    if code.is_empty() {
        return Err(AppError::Auth("Authorization code cannot be empty".to_string()));
    }
    if let Some(s) = pasted_state
        && s != state
    {
        return Err(AppError::Auth("OAuth state mismatch".to_string()));
    }
    Ok((code, REDIRECT_URI.to_string()))
}
