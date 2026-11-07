//! OAuth 2.0 PKCE and Device Code login and refresh handlers.

mod chatgpt;
pub mod jwt;
mod openrouter;

#[cfg(test)]
mod tests;

pub use jwt::extract_chatgpt_account_id;

use crate::auth::loopback::{CallbackParams, LoopbackServer};
use crate::auth::pkce::{PkceChallenge, generate_state};
use rho_harness_core::auth::{OAuthLoginCallbacks, StoredCredential};
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::provider::ProviderId;
use std::time::Duration;

pub(crate) fn oauth_credential(
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    (account_id, account_email): (Option<String>, Option<String>),
) -> StoredCredential {
    let expires_at_ms = expires_in.map(|sec| chrono::Utc::now().timestamp_millis() + sec * 1000 - 5 * 60 * 1000);
    StoredCredential::OAuth {
        access_token,
        refresh_token,
        expires_at_ms,
        account_id,
        account_email,
    }
}

pub(crate) async fn refresh_oauth_credential<Exchange, Fut>(
    credential: &StoredCredential,
    expired_message: &str,
    exchange: Exchange,
) -> Result<StoredCredential>
where
    Exchange: FnOnce(String, (Option<String>, Option<String>)) -> Fut,
    Fut: Future<Output = Result<StoredCredential>>,
{
    let StoredCredential::OAuth {
        refresh_token: Some(refresh),
        account_id,
        account_email,
        ..
    } = credential
    else {
        return Err(AppError::Auth(expired_message.to_string()));
    };
    exchange(refresh.clone(), (account_id.clone(), account_email.clone())).await
}

pub(crate) fn validate_callback(callback: CallbackParams, state: &str) -> Result<String> {
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
    Ok(code)
}

/// Provider-specific parameters of a PKCE loopback authorization.
pub(crate) struct LoginPrompt {
    pub port: u16,
    pub callback_timeout: Duration,
    pub hint: &'static str,
    pub progress: &'static str,
}

/// Validated authorization code plus the PKCE verifier needed to redeem it.
pub(crate) struct AuthorizationGrant {
    pub code: String,
    pub verifier: String,
}

/// Run the PKCE loopback login shared by browser providers: bind, show the authorize URL, wait
/// for the redirect, validate it against the state, then hand the grant to `redeem`.
pub(crate) async fn loopback_login<T, Fut>(
    callbacks: &dyn OAuthLoginCallbacks,
    prompt: &LoginPrompt,
    build_auth_url: fn(&str, &str) -> String,
    redeem: impl FnOnce(AuthorizationGrant) -> Fut,
) -> Result<T>
where
    Fut: Future<Output = Result<T>>,
{
    let pkce = PkceChallenge::generate();
    let state = generate_state();
    let auth_url = build_auth_url(&pkce.challenge, &state);
    let server =
        LoopbackServer::bind_for_login(callbacks, prompt.port, &auth_url, prompt.hint, prompt.progress).await?;
    let callback = server.wait_for_callback(prompt.callback_timeout).await?;
    let code = validate_callback(callback, &state)?;
    callbacks
        .on_progress("Exchanging authorization code for tokens...")
        .await?;
    redeem(AuthorizationGrant {
        code,
        verifier: pkce.verifier,
    })
    .await
}

enum OAuthOperation<'a> {
    Login(&'a dyn OAuthLoginCallbacks),
    Refresh {
        credential: &'a StoredCredential,
        refresh_token: &'a str,
    },
}

async fn dispatch_oauth(provider: ProviderId, operation: OAuthOperation<'_>) -> Result<StoredCredential> {
    use OAuthOperation::{Login, Refresh};
    match (provider, operation) {
        (ProviderId::ChatGpt, Login(callbacks)) => chatgpt::perform_openai_pkce(callbacks).await,
        (ProviderId::OpenRouter, Login(callbacks)) => openrouter::perform_openrouter_pkce(callbacks).await,
        (ProviderId::Antigravity, Login(callbacks)) => super::antigravity::perform_login(callbacks).await,
        (ProviderId::ClaudeCode, Login(callbacks)) => super::claude::perform_login(callbacks).await,
        (_, Login(_)) => Err(AppError::Auth(format!(
            "OAuth login is not supported for provider '{provider}'"
        ))),
        (ProviderId::ChatGpt, Refresh { refresh_token, .. }) => chatgpt::refresh_openai_token(refresh_token).await,
        (ProviderId::Antigravity, Refresh { credential, .. }) => {
            super::antigravity::refresh_credential(credential).await
        }
        (ProviderId::ClaudeCode, Refresh { credential, .. }) => super::claude::refresh_credential(credential).await,
        (_, Refresh { credential, .. }) => Ok(credential.clone()),
    }
}

pub async fn perform_oauth_login(
    provider: ProviderId,
    callbacks: &dyn OAuthLoginCallbacks,
) -> Result<StoredCredential> {
    dispatch_oauth(provider, OAuthOperation::Login(callbacks)).await
}

pub async fn refresh_oauth_token(provider: ProviderId, credential: &StoredCredential) -> Result<StoredCredential> {
    match credential {
        StoredCredential::ApiKey { .. } => Ok(credential.clone()),
        StoredCredential::OAuth {
            refresh_token: Some(refresh),
            ..
        } => {
            let operation = OAuthOperation::Refresh {
                credential,
                refresh_token: refresh,
            };
            dispatch_oauth(provider, operation).await
        }
        StoredCredential::OAuth { .. } => Err(AppError::Auth(format!(
            "OAuth token for '{provider}' has expired and has no refresh token. Please re-run /login."
        ))),
    }
}
