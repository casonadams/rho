pub mod client;
pub mod flow;
pub mod local;
#[cfg(test)]
mod tests;

pub use client::{
    AUTHORIZE_URL, CLIENT_ID, ClaudeProfileAccount, ClaudeProfileOrganization, ClaudeProfileResponse,
    ClaudeTokenResponse, PROFILE_URL, REDIRECT_URI, SCOPES, TOKEN_TIMEOUT, USER_AGENT, exchange_code,
    exchange_code_with_redirect, fetch_profile, refresh_token,
};
pub use flow::{
    CALLBACK_TIMEOUT, DEFAULT_LOOPBACK_PORT, acquire_auth_code, build_authorize_url, parse_auth_code_and_state,
};
pub use local::{detect_local_claude_credentials, detect_local_claude_credentials_async};

use crate::auth::pkce::{PkceChallenge, generate_state};
use rho_harness_core::auth::{OAuthLoginCallbacks, SelectOption, StoredCredential};
use rho_harness_core::error::{AppError, Result};

fn make_oauth_cred(
    token: ClaudeTokenResponse,
    account_id: Option<String>,
    account_email: Option<String>,
) -> StoredCredential {
    let expires_at_ms = token
        .expires_in
        .map(|sec| chrono::Utc::now().timestamp_millis() + sec * 1000 - 5 * 60 * 1000);
    let mut cred = StoredCredential::oauth(token.access_token, token.refresh_token, expires_at_ms);
    if let StoredCredential::OAuth {
        account_id: ref mut id_slot,
        account_email: ref mut email_slot,
        ..
    } = cred
    {
        *id_slot = account_id;
        *email_slot = account_email;
    }
    cred
}

pub async fn perform_login(callbacks: &dyn OAuthLoginCallbacks) -> Result<StoredCredential> {
    if let Some(local_cred) = detect_local_claude_credentials_async().await {
        let email = match &local_cred {
            StoredCredential::OAuth {
                account_email: Some(e), ..
            } => e.clone(),
            _ => "local installation".to_string(),
        };
        let options = [
            SelectOption::new("import", format!("Import existing credentials ({email})")),
            SelectOption::new("browser", "Sign in with a new browser session"),
        ];
        let choice = callbacks
            .on_select(
                &format!("Found existing Claude Code credentials for {email}. Import these credentials?"),
                &options,
            )
            .await?;
        if choice.as_deref() == Some("import") {
            callbacks
                .on_progress("Imported existing Claude Code credentials.")
                .await?;
            return Ok(local_cred);
        }
    }

    let pkce = PkceChallenge::generate();
    let state = generate_state();
    let (code, redirect_uri) = acquire_auth_code(callbacks, &pkce.challenge, &state).await?;

    callbacks
        .on_progress("Exchanging authorization code for tokens...")
        .await?;

    let token = exchange_code_with_redirect(&code, &pkce.verifier, &redirect_uri).await?;
    let profile = fetch_profile(&token.access_token).await;
    let account_id = profile
        .as_ref()
        .and_then(|p| p.organization.as_ref())
        .and_then(|o| o.uuid.clone())
        .or_else(|| token.organization.as_ref().and_then(|o| o.uuid.clone()))
        .or_else(|| token.account.as_ref().and_then(|a| a.uuid.clone()));
    let account_email = profile
        .as_ref()
        .and_then(|p| p.account.as_ref())
        .and_then(|a| a.email_address.clone())
        .or_else(|| token.account.as_ref().and_then(|a| a.email_address.clone()));

    Ok(make_oauth_cred(token, account_id, account_email))
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
            "Claude OAuth token has expired and has no refresh token. Re-run 'rho login claude'.".to_string(),
        ));
    };

    let mut token = refresh_token(refresh).await?;
    if token.refresh_token.is_none() {
        token.refresh_token = Some(refresh.clone());
    }
    let new_account_id = token
        .organization
        .as_ref()
        .and_then(|o| o.uuid.clone())
        .or_else(|| token.account.as_ref().and_then(|a| a.uuid.clone()))
        .or_else(|| account_id.clone());
    let new_email = token
        .account
        .as_ref()
        .and_then(|a| a.email_address.clone())
        .or_else(|| account_email.clone());

    Ok(make_oauth_cred(token, new_account_id, new_email))
}
