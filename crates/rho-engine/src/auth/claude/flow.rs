use super::client::{AUTHORIZE_URL, CLIENT_ID, REDIRECT_URI, SCOPES};
use rho_harness_core::auth::OAuthLoginCallbacks;
use rho_harness_core::error::{AppError, Result};
use url::Url;

pub fn build_authorize_url(redirect_uri: &str, challenge: &str, state: &str) -> String {
    let mut url = Url::parse(AUTHORIZE_URL).expect("valid authorize url");
    url.query_pairs_mut()
        .append_pair("code", "true")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", SCOPES)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state);
    url.to_string()
}

fn parse_query_code_state(query: &str) -> (Option<String>, Option<String>) {
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
    (code, state)
}

pub fn parse_auth_code_and_state(input: &str) -> (String, Option<String>) {
    let trimmed = input.trim();
    if let Some((_, query)) = trimmed.split_once('?') {
        let (code, state) = parse_query_code_state(query);
        if let Some(c) = code {
            return (c, state);
        }
    }
    if let Some((c, s)) = trimmed.split_once('#') {
        return (c.trim().to_string(), Some(s.trim().to_string()));
    }
    (trimmed.to_string(), None)
}

pub async fn acquire_auth_code(
    callbacks: &dyn OAuthLoginCallbacks,
    challenge: &str,
    state: &str,
) -> Result<(String, String)> {
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
