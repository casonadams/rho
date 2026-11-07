pub mod client;
pub mod flow;
pub mod local;
#[cfg(test)]
mod tests;

use client::TOKEN_URL;
pub use client::{
    ANTHROPIC_BETA, AUTHORIZE_URL, CLIENT_ID, ClaudeProfileAccount, ClaudeProfileOrganization, ClaudeProfileResponse,
    ClaudeTokenResponse, PROFILE_URL, REDIRECT_URI, SCOPES, TOKEN_TIMEOUT, USER_AGENT, exchange_code_with_redirect,
    fetch_profile, refresh_token,
};
pub use flow::{acquire_auth_code, build_authorize_url, parse_auth_code_and_state};
pub use local::{detect_local_claude_credentials, detect_local_claude_credentials_async};

use super::oauth::{oauth_credential, refresh_oauth_credential};
use crate::auth::pkce::{PkceChallenge, generate_state};
use rho_harness_core::auth::{OAuthLoginCallbacks, SelectOption, StoredCredential};
use rho_harness_core::error::Result;

async fn try_import_local_credentials(callbacks: &dyn OAuthLoginCallbacks) -> Result<Option<StoredCredential>> {
    let local_cred = detect_local_claude_credentials_async().await;
    confirm_import_local_credentials(local_cred, callbacks).await
}

pub(crate) async fn confirm_import_local_credentials(
    local_cred: Option<StoredCredential>,
    callbacks: &dyn OAuthLoginCallbacks,
) -> Result<Option<StoredCredential>> {
    let Some(local_cred) = local_cred else {
        return Ok(None);
    };
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
    let msg = format!("Found existing Claude Code credentials for {email}. Import these credentials?");
    let choice = callbacks.on_select(&msg, &options).await?;
    if choice.as_deref() == Some("import") {
        callbacks
            .on_progress("Imported existing Claude Code credentials.")
            .await?;
        return Ok(Some(local_cred));
    }
    Ok(None)
}

fn resolve_account_ids(
    token: &ClaudeTokenResponse,
    profile: &Option<ClaudeProfileResponse>,
) -> (Option<String>, Option<String>) {
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
    (account_id, account_email)
}

async fn exchange_and_resolve_profile(
    code: &str,
    verifier: &str,
    redirect_uri: &str,
    state: Option<&str>,
) -> Result<StoredCredential> {
    let token = exchange_code_with_redirect(TOKEN_URL, code, verifier, redirect_uri, state).await?;
    let profile = fetch_profile(PROFILE_URL, &token.access_token).await;
    let (account_id, account_email) = resolve_account_ids(&token, &profile);
    Ok(oauth_credential(
        token.access_token,
        token.refresh_token,
        token.expires_in,
        (account_id, account_email),
    ))
}

pub async fn perform_login(callbacks: &dyn OAuthLoginCallbacks) -> Result<StoredCredential> {
    if let Some(cred) = try_import_local_credentials(callbacks).await? {
        return Ok(cred);
    }

    let pkce = PkceChallenge::generate();
    let state = generate_state();
    let (code, redirect_uri) = acquire_auth_code(callbacks, &pkce.challenge, &state).await?;

    callbacks
        .on_progress("Exchanging authorization code for tokens...")
        .await?;

    exchange_and_resolve_profile(&code, &pkce.verifier, &redirect_uri, Some(&state)).await
}

fn resolve_refreshed_ids(
    token: &ClaudeTokenResponse,
    (account_id, account_email): (Option<String>, Option<String>),
) -> (Option<String>, Option<String>) {
    let new_account_id = token
        .organization
        .as_ref()
        .and_then(|o| o.uuid.clone())
        .or_else(|| token.account.as_ref().and_then(|a| a.uuid.clone()))
        .or(account_id);
    let new_email = token
        .account
        .as_ref()
        .and_then(|a| a.email_address.clone())
        .or(account_email);
    (new_account_id, new_email)
}

async fn exchange_refresh_token(
    token_url: &str,
    refresh: String,
    (account_id, account_email): (Option<String>, Option<String>),
) -> Result<StoredCredential> {
    let mut token = refresh_token(token_url, &refresh).await?;
    if token.refresh_token.is_none() {
        token.refresh_token = Some(refresh);
    }
    let (new_account_id, new_email) = resolve_refreshed_ids(&token, (account_id, account_email));
    Ok(oauth_credential(
        token.access_token,
        token.refresh_token,
        token.expires_in,
        (new_account_id, new_email),
    ))
}

pub async fn refresh_credential(credential: &StoredCredential) -> Result<StoredCredential> {
    refresh_oauth_credential(
        credential,
        "Claude OAuth token has expired and has no refresh token. Re-run 'rho login claude'.",
        |refresh, account| exchange_refresh_token(TOKEN_URL, refresh, account),
    )
    .await
}
