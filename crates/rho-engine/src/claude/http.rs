//! Shared Claude HTTP plumbing: Anthropic Messages endpoint, client,
//! request headers, and friendly error mapping.

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::sync::LazyLock;

pub const DEFAULT_ENDPOINT: &str = "https://api.anthropic.com";
pub const MESSAGES_PATH: &str = "/v1/messages";
pub const PROVIDER_NAME: &str = "claude";

pub const ANTHROPIC_VERSION: &str = "2023-06-01";
pub const ANTHROPIC_BETA: &str = "claude-code-20250219,oauth-2025-04-20";
pub const USER_AGENT: &str = "claude-cli/2.1.62";

static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    crate::install_crypto_provider();
    reqwest::Client::builder().no_proxy().build().unwrap_or_default()
});

pub fn http_client() -> &'static reqwest::Client {
    &HTTP_CLIENT
}

/// Construct headers Anthropic Messages API expects for Claude Code OAuth.
pub fn claude_headers(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}")) {
        headers.insert(reqwest::header::AUTHORIZATION, value);
    }
    headers.insert(
        reqwest::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        HeaderName::from_static("anthropic-version"),
        HeaderValue::from_static(ANTHROPIC_VERSION),
    );
    headers.insert(
        HeaderName::from_static("anthropic-beta"),
        HeaderValue::from_static(ANTHROPIC_BETA),
    );
    headers.insert(reqwest::header::USER_AGENT, HeaderValue::from_static(USER_AGENT));
    headers
}

pub fn friendly_error(status: Option<u16>, body: &str) -> String {
    let message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .map(String::from)
        })
        .unwrap_or_else(|| body.chars().take(300).collect());

    match status {
        Some(401) => "Claude OAuth session expired or credentials are invalid. Run 'rho login claude'.".to_string(),
        Some(429) => format!("Claude rate limit or usage limit reached. Wait a bit and retry. Backend: {message}"),
        Some(403) => format!("Claude access denied. Backend: {message}"),
        Some(400) => format!("Claude request invalid. Backend: {message}"),
        Some(529) => "Anthropic API is overloaded. Wait a bit and retry.".to_string(),
        Some(other) => format!("Claude API error ({other}): {message}"),
        None => format!("Claude request failed: {message}"),
    }
}
