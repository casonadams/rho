//! Google OAuth for the Antigravity (Cloud Code Assist) provider.
//!
//! Mirrors the flow pi-antigravity uses: Google's public Antigravity desktop
//! OAuth client, PKCE S256, fixed loopback redirect on port 51121, then Cloud
//! Code Assist project discovery over the `v1internal` endpoints.

mod client;
#[cfg(test)]
mod tests;

use super::oauth::{AuthorizationGrant, LoginPrompt, loopback_login, oauth_credential, refresh_oauth_credential};
use crate::antigravity::load_project_id;
use client::{
    CALLBACK_TIMEOUT, GOOGLE_AUTH_URL, GOOGLE_CLIENT_ID, GOOGLE_TOKEN_URL, GOOGLE_USERINFO_URL, GoogleTokenResponse,
    REDIRECT_PORT, REDIRECT_URI_ENCODED, SCOPES, exchange_code, fetch_user_email, refresh_google_token,
};
use rho_harness_core::auth::{OAuthLoginCallbacks, StoredCredential};
use rho_harness_core::error::Result;
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

async fn build_antigravity_credential(token: GoogleTokenResponse) -> StoredCredential {
    let email = fetch_user_email(GOOGLE_USERINFO_URL, &token.access_token).await;
    let project_id = load_project_id(&token.access_token)
        .await
        .unwrap_or_else(|| stable_project_id(email.as_deref().unwrap_or("antigravity-default")));
    oauth_credential(
        token.access_token,
        token.refresh_token,
        token.expires_in,
        (Some(project_id), email),
    )
}

fn build_google_auth_url(challenge: &str, state: &str) -> String {
    format!(
        "{GOOGLE_AUTH_URL}?response_type=code&client_id={GOOGLE_CLIENT_ID}&redirect_uri={REDIRECT_URI_ENCODED}&scope={SCOPES}&code_challenge={challenge}&code_challenge_method=S256&state={state}&access_type=offline&prompt=consent"
    )
}

const GOOGLE_LOGIN_PROMPT: LoginPrompt = LoginPrompt {
    port: REDIRECT_PORT,
    callback_timeout: CALLBACK_TIMEOUT,
    hint: "Complete Google sign-in to finish.",
    progress: "Waiting for Google authorization...",
};

async fn redeem_google_grant(grant: AuthorizationGrant) -> Result<StoredCredential> {
    let token = exchange_code(GOOGLE_TOKEN_URL, &grant.code, &grant.verifier).await?;
    Ok(build_antigravity_credential(token).await)
}

pub async fn perform_login(callbacks: &dyn OAuthLoginCallbacks) -> Result<StoredCredential> {
    loopback_login(
        callbacks,
        &GOOGLE_LOGIN_PROMPT,
        build_google_auth_url,
        redeem_google_grant,
    )
    .await
}

async fn exchange_refresh_token(
    token_url: &str,
    refresh: String,
    account: (Option<String>, Option<String>),
) -> Result<StoredCredential> {
    let token = refresh_google_token(token_url, &refresh).await?;
    let refresh_token = token.refresh_token.or(Some(refresh));
    Ok(oauth_credential(
        token.access_token,
        refresh_token,
        token.expires_in,
        account,
    ))
}

pub async fn refresh_credential(credential: &StoredCredential) -> Result<StoredCredential> {
    refresh_oauth_credential(
        credential,
        "Antigravity token has expired and has no refresh token. Re-run 'rho login antigravity'.",
        |refresh, account| exchange_refresh_token(GOOGLE_TOKEN_URL, refresh, account),
    )
    .await
}
