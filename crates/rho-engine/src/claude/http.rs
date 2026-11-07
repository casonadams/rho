//! Shared Claude HTTP plumbing: Anthropic Messages endpoint, request headers for
//! login tokens and API keys, and friendly error mapping.

use crate::auth::claude::{ANTHROPIC_BETA, USER_AGENT};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};

pub const DEFAULT_ENDPOINT: &str = "https://api.anthropic.com";
pub const MESSAGES_PATH: &str = "/v1/messages";
pub const PROVIDER_NAME: &str = "claude";

pub const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeAuth {
    Login,
    ApiKey,
}

pub fn claude_headers(auth: ClaudeAuth, token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        HeaderName::from_static("anthropic-version"),
        HeaderValue::from_static(ANTHROPIC_VERSION),
    );
    match auth {
        ClaudeAuth::ApiKey => {
            if let Ok(value) = HeaderValue::from_str(token) {
                headers.insert(HeaderName::from_static("x-api-key"), value);
            }
        }
        ClaudeAuth::Login => {
            if let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}")) {
                headers.insert(AUTHORIZATION, value);
            }
            headers.insert(
                HeaderName::from_static("anthropic-beta"),
                HeaderValue::from_static(ANTHROPIC_BETA),
            );
            headers.insert(
                HeaderName::from_static("anthropic-dangerous-direct-browser-access"),
                HeaderValue::from_static("true"),
            );
            headers.insert(HeaderName::from_static("x-app"), HeaderValue::from_static("cli"));
            headers.insert(reqwest::header::USER_AGENT, HeaderValue::from_static(USER_AGENT));
        }
    }
    headers
}

pub fn friendly_error(status: Option<u16>, body: &str) -> String {
    crate::http::friendly_oauth_error("Claude", "claude", status, body)
}
