//! Shared Antigravity HTTP plumbing: Cloud Code Assist endpoints, static
//! client, request headers, error mapping, and the metadata POST helper.

use reqwest::header::{HeaderMap, HeaderValue};
use std::sync::LazyLock;
use std::time::Duration;

pub const DEFAULT_ENDPOINT: &str = "https://daily-cloudcode-pa.googleapis.com";
pub const ENDPOINT_CANDIDATES: [&str; 3] = [
    DEFAULT_ENDPOINT,
    "https://daily-cloudcode-pa.sandbox.googleapis.com",
    "https://cloudcode-pa.googleapis.com",
];

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);
pub(super) const PROVIDER_NAME: &str = "antigravity";

static HTTP_CLIENT: LazyLock<reqwest::Client> =
    LazyLock::new(|| reqwest::Client::builder().no_proxy().build().unwrap_or_default());

pub fn http_client() -> &'static reqwest::Client {
    &HTTP_CLIENT
}

/// Headers Cloud Code Assist expects on every call (pi-antigravity parity).
pub fn antigravity_headers(token: &str) -> HeaderMap {
    let platform = match std::env::consts::OS {
        "macos" => "MACOS",
        "windows" => "WINDOWS",
        _ => "LINUX",
    };
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}")) {
        headers.insert("Authorization", value);
    }
    headers.insert("Content-Type", HeaderValue::from_static("application/json"));
    headers.insert(
        "User-Agent",
        HeaderValue::from_static("antigravity/hub/2.8.0 (aidev_client; os_type=darwin; arch=arm64; cl=963137146)"),
    );
    headers.insert(
        "X-Goog-Api-Client",
        HeaderValue::from_static("google-cloud-sdk vscode_cloudshelleditor/0.1"),
    );
    if let Ok(metadata) = HeaderValue::from_str(&format!(
        r#"{{"ideType":"ANTIGRAVITY","platform":"{platform}","pluginType":"GEMINI"}}"#
    )) {
        headers.insert("Client-Metadata", metadata);
    }
    headers
}

pub(super) fn friendly_error(status: Option<u16>, body: &str) -> String {
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
        Some(429) if message.contains("Individual quota reached") => {
            let reset = message
                .split("Resets in ")
                .nth(1)
                .map(|r| r.trim_end_matches('.'))
                .unwrap_or("unknown");
            format!("Antigravity quota reached. Resets in {reset}. Switch models or wait for the reset.")
        }
        Some(429) => "Antigravity rate limit reached. Wait a bit and retry.".to_string(),
        Some(401) => "Antigravity login expired or credentials are invalid. Run 'rho login antigravity'.".to_string(),
        Some(403) => format!("Antigravity access denied. Re-login or try another model. Backend: {message}"),
        Some(404) => format!("Model not available on Antigravity. Backend: {message}"),
        Some(503) if message.contains("No capacity") => {
            "This model has no capacity right now. Try another model.".to_string()
        }
        Some(other) => format!("Antigravity API error ({other}): {message}"),
        None => format!("Antigravity request failed: {message}"),
    }
}

/// POST a Cloud Code Assist metadata endpoint, trying endpoint candidates.
pub(crate) async fn post_metadata(path: &str, token: &str, body: serde_json::Value) -> Option<serde_json::Value> {
    for endpoint in ENDPOINT_CANDIDATES {
        let response = http_client()
            .post(format!("{endpoint}{path}"))
            .headers(antigravity_headers(token))
            .json(&body)
            .timeout(DISCOVERY_TIMEOUT)
            .send()
            .await;
        if let Ok(response) = response
            && response.status().is_success()
            && let Ok(json) = response.json::<serde_json::Value>().await
        {
            return Some(json);
        }
    }
    None
}
