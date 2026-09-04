//! Google OAuth for the Antigravity (Cloud Code Assist) provider.
//!
//! Mirrors the flow pi-antigravity uses: Google's public Antigravity desktop
//! OAuth client, PKCE S256, fixed loopback redirect on port 51121, then Cloud
//! Code Assist project discovery over the `v1internal` endpoints.

mod client;
#[cfg(test)]
mod tests;

use super::loopback::LoopbackServer;
use super::pkce::{PkceChallenge, generate_state};
use crate::antigravity::load_project_id;
use client::{
    CALLBACK_TIMEOUT, GOOGLE_AUTH_URL, GOOGLE_CLIENT_ID, REDIRECT_PORT, REDIRECT_URI_ENCODED, SCOPES, exchange_code,
    fetch_user_email, refresh_google_token,
};
use rho_harness_core::auth::{OAuthLoginCallbacks, StoredCredential};
use rho_harness_core::error::{AppError, Result};
use sha2::{Digest, Sha256};

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

    let token = refresh_google_token(refresh).await?;

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
