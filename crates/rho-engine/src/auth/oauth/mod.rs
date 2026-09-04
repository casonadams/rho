//! OAuth 2.0 PKCE and Device Code login and refresh handlers.

mod chatgpt;
mod copilot;
pub mod jwt;
mod openrouter;

#[cfg(test)]
mod tests;

pub use jwt::extract_chatgpt_account_id;

use rho_harness_core::auth::{OAuthLoginCallbacks, StoredCredential};
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::provider::ProviderId;

pub(super) use super::http::http_client;

pub async fn perform_oauth_login(
    provider: ProviderId,
    callbacks: &dyn OAuthLoginCallbacks,
) -> Result<StoredCredential> {
    match provider {
        ProviderId::ChatGpt => chatgpt::perform_openai_pkce(callbacks).await,
        ProviderId::Copilot => copilot::perform_copilot_device_flow(callbacks).await,
        ProviderId::OpenRouter => openrouter::perform_openrouter_pkce(callbacks).await,
        ProviderId::Antigravity => super::antigravity::perform_login(callbacks).await,
        _ => Err(AppError::Auth(format!(
            "OAuth login is not supported for provider '{provider}'"
        ))),
    }
}

pub async fn refresh_oauth_token(provider: ProviderId, credential: &StoredCredential) -> Result<StoredCredential> {
    match credential {
        StoredCredential::ApiKey { .. } => Ok(credential.clone()),
        StoredCredential::OAuth {
            refresh_token: Some(refresh),
            ..
        } => match provider {
            ProviderId::ChatGpt => chatgpt::refresh_openai_token(refresh).await,
            ProviderId::Copilot => copilot::refresh_copilot_token(refresh).await,
            ProviderId::Antigravity => super::antigravity::refresh_credential(credential).await,
            _ => Ok(credential.clone()),
        },
        StoredCredential::OAuth { .. } => Err(AppError::Auth(format!(
            "OAuth token for '{provider}' has expired and has no refresh token. Please re-run /login."
        ))),
    }
}
