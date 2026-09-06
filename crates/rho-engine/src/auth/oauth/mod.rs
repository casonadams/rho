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

fn dispatch_oauth_login<'a>(
    provider: ProviderId,
    callbacks: &'a dyn OAuthLoginCallbacks,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<StoredCredential>> + Send + 'a>> {
    match provider {
        ProviderId::ChatGpt => Box::pin(chatgpt::perform_openai_pkce(callbacks)),
        ProviderId::Copilot => Box::pin(copilot::perform_copilot_device_flow(callbacks)),
        ProviderId::OpenRouter => Box::pin(openrouter::perform_openrouter_pkce(callbacks)),
        ProviderId::Antigravity => Box::pin(super::antigravity::perform_login(callbacks)),
        ProviderId::ClaudeCode => Box::pin(super::claude::perform_login(callbacks)),
        _ => Box::pin(async move {
            Err(AppError::Auth(format!(
                "OAuth login is not supported for provider '{provider}'"
            )))
        }),
    }
}

pub async fn perform_oauth_login(
    provider: ProviderId,
    callbacks: &dyn OAuthLoginCallbacks,
) -> Result<StoredCredential> {
    dispatch_oauth_login(provider, callbacks).await
}

fn dispatch_oauth_refresh<'a>(
    provider: ProviderId,
    credential: &'a StoredCredential,
    refresh: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<StoredCredential>> + Send + 'a>> {
    match provider {
        ProviderId::ChatGpt => Box::pin(chatgpt::refresh_openai_token(refresh)),
        ProviderId::Copilot => Box::pin(copilot::refresh_copilot_token(refresh)),
        ProviderId::Antigravity => Box::pin(super::antigravity::refresh_credential(credential)),
        ProviderId::ClaudeCode => Box::pin(super::claude::refresh_credential(credential)),
        _ => Box::pin(async move { Ok(credential.clone()) }),
    }
}

pub async fn refresh_oauth_token(provider: ProviderId, credential: &StoredCredential) -> Result<StoredCredential> {
    match credential {
        StoredCredential::ApiKey { .. } => Ok(credential.clone()),
        StoredCredential::OAuth {
            refresh_token: Some(refresh),
            ..
        } => dispatch_oauth_refresh(provider, credential, refresh).await,
        StoredCredential::OAuth { .. } => Err(AppError::Auth(format!(
            "OAuth token for '{provider}' has expired and has no refresh token. Please re-run /login."
        ))),
    }
}
