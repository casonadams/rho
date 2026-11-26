//! Shared Claude HTTP plumbing: Anthropic Messages endpoint, request headers for
//! login tokens and API keys, and friendly error mapping.

use crate::auth::claude::{ANTHROPIC_BETA, USER_AGENT};
use crate::http::{bearer, header_value};
use crate::provider::error::HttpFailure;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};

pub const DEFAULT_ENDPOINT: &str = "https://api.anthropic.com";
pub const MESSAGES_PATH: &str = "/v1/messages";

pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Login requests ignore the per-tool `eager_input_streaming` flag unless this beta is also sent.
const FINE_GRAINED_TOOL_STREAMING_BETA: &str = "fine-grained-tool-streaming-2025-05-14";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeAuth {
    Login,
    ApiKey,
}

pub fn claude_headers(auth: ClaudeAuth, token: &str) -> Result<HeaderMap, HttpFailure> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        HeaderName::from_static("anthropic-version"),
        HeaderValue::from_static(ANTHROPIC_VERSION),
    );
    match auth {
        ClaudeAuth::ApiKey => {
            headers.insert(
                HeaderName::from_static("x-api-key"),
                header_value(token, "The API key")?,
            );
        }
        ClaudeAuth::Login => {
            headers.insert(AUTHORIZATION, bearer(token)?);
            headers.insert(
                HeaderName::from_static("anthropic-beta"),
                header_value(
                    &format!("{ANTHROPIC_BETA},{FINE_GRAINED_TOOL_STREAMING_BETA}"),
                    "The beta list",
                )?,
            );
            headers.insert(
                HeaderName::from_static("anthropic-dangerous-direct-browser-access"),
                HeaderValue::from_static("true"),
            );
            headers.insert(HeaderName::from_static("x-app"), HeaderValue::from_static("cli"));
            headers.insert(reqwest::header::USER_AGENT, HeaderValue::from_static(USER_AGENT));
        }
    }
    Ok(headers)
}

pub fn friendly_error(status: Option<u16>, body: &str) -> String {
    crate::http::friendly_oauth_error("Claude", "claude", status, body)
}
