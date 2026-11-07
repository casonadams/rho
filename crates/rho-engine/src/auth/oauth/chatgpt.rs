//! OpenAI / ChatGPT PKCE OAuth login and token refresh flow.

use super::jwt::extract_chatgpt_account_id;
use super::{AuthorizationGrant, LoginPrompt, loopback_login};
use crate::auth::pkce::AuthorizeRequest;
use crate::auth::token_request::send_for_json;
use crate::http::CLIENT;
use rho_harness_core::auth::{OAuthLoginCallbacks, StoredCredential};
use rho_harness_core::error::Result;
use serde::Deserialize;
use std::time::Duration;

const OPENAI_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_AUTH_URL: &str = "https://auth.openai.com/oauth/authorize";
const OPENAI_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const OPENAI_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const OPENAI_SCOPE: &str = "openid profile email offline_access";

#[derive(Debug, Deserialize)]
pub(crate) struct TokenResponse {
    pub(crate) access_token: String,
    pub(crate) refresh_token: Option<String>,
    pub(crate) expires_in: Option<i64>,
}

async fn post_openai_token_form(form: &[(&str, &str)], operation: &str) -> Result<TokenResponse> {
    let request = CLIENT
        .post(OPENAI_TOKEN_URL)
        .timeout(Duration::from_secs(15))
        .form(form);
    send_for_json(request, operation).await
}

async fn exchange_openai_code(code: &str, verifier: &str) -> Result<TokenResponse> {
    let form = [
        ("grant_type", "authorization_code"),
        ("client_id", OPENAI_CLIENT_ID),
        ("code", code),
        ("code_verifier", verifier),
        ("redirect_uri", OPENAI_REDIRECT_URI),
    ];
    post_openai_token_form(&form, "OpenAI token exchange").await
}

pub(crate) fn build_openai_credential(token_data: TokenResponse) -> StoredCredential {
    let expires_at_ms = token_data
        .expires_in
        .map(|sec| chrono::Utc::now().timestamp_millis() + sec * 1000);
    let account_id = extract_chatgpt_account_id(&token_data.access_token);
    let mut cred = StoredCredential::oauth(token_data.access_token, token_data.refresh_token, expires_at_ms);
    if let StoredCredential::OAuth {
        account_id: ref mut id_field,
        ..
    } = cred
    {
        *id_field = account_id;
    }
    cred
}

pub fn build_openai_auth_url(challenge: &str, state: &str) -> String {
    AuthorizeRequest {
        endpoint: OPENAI_AUTH_URL,
        client_id: OPENAI_CLIENT_ID,
        redirect_uri: OPENAI_REDIRECT_URI,
        scope: OPENAI_SCOPE,
        challenge,
        state,
        extra: &[
            ("id_token_add_organizations", "true"),
            ("codex_cli_simplified_flow", "true"),
            ("originator", "rho"),
        ],
    }
    .to_url()
}

const OPENAI_LOGIN_PROMPT: LoginPrompt = LoginPrompt {
    port: 1455,
    callback_timeout: Duration::from_secs(120),
    hint: "A browser window will open. Complete login to finish.",
    progress: "Waiting for browser authorization...",
};

async fn redeem_openai_grant(grant: AuthorizationGrant) -> Result<StoredCredential> {
    let token = exchange_openai_code(&grant.code, &grant.verifier).await?;
    Ok(build_openai_credential(token))
}

pub async fn perform_openai_pkce(callbacks: &dyn OAuthLoginCallbacks) -> Result<StoredCredential> {
    loopback_login(
        callbacks,
        &OPENAI_LOGIN_PROMPT,
        build_openai_auth_url,
        redeem_openai_grant,
    )
    .await
}

async fn post_refresh_request(refresh_token: &str) -> Result<TokenResponse> {
    let form = [
        ("grant_type", "refresh_token"),
        ("client_id", OPENAI_CLIENT_ID),
        ("refresh_token", refresh_token),
    ];
    post_openai_token_form(&form, "OpenAI token refresh").await
}

pub async fn refresh_openai_token(refresh_token: &str) -> Result<StoredCredential> {
    let mut token = post_refresh_request(refresh_token).await?;
    if token.refresh_token.is_none() {
        token.refresh_token = Some(refresh_token.to_string());
    }
    Ok(build_openai_credential(token))
}
