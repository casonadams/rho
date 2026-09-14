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

static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    crate::install_crypto_provider();
    reqwest::Client::builder().no_proxy().build().unwrap_or_default()
});

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

fn parse_error_message(body: &str) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return body.chars().take(300).collect();
    };
    let Some(err) = v.get("error") else {
        return body.chars().take(300).collect();
    };
    let message = err.get("message").and_then(|m| m.as_str()).unwrap_or("unknown error");
    if let Some(details) = err.get("details").and_then(|d| d.as_array()) {
        let mut violations = Vec::new();
        for item in details {
            if let Some(fvs) = item.get("fieldViolations").and_then(|f| f.as_array()) {
                for fv in fvs {
                    let field = fv.get("field").and_then(|f| f.as_str()).unwrap_or("");
                    let desc = fv.get("description").and_then(|d| d.as_str()).unwrap_or("");
                    if !field.is_empty() || !desc.is_empty() {
                        violations.push(format!("{field}: {desc}"));
                    }
                }
            }
        }
        if !violations.is_empty() {
            return format!("{message} (details: {})", violations.join("; "));
        }
        return format!("{message} (raw details: {details:?})");
    }
    format!("{message} (body: {v})")
}

fn format_status_error(status: u16, message: &str) -> String {
    match status {
        429 if message.contains("Individual quota reached") => {
            let reset = message
                .split("Resets in ")
                .nth(1)
                .map(|r| r.trim_end_matches('.'))
                .unwrap_or("unknown");
            format!("Antigravity quota reached. Resets in {reset}. Switch models or wait for the reset.")
        }
        429 => "Antigravity rate limit reached. Wait a bit and retry.".to_string(),
        401 => "Antigravity login expired or credentials are invalid. Run 'rho login antigravity'.".to_string(),
        403 => format!("Antigravity access denied. Re-login or try another model. Backend: {message}"),
        404 => format!("Model not available on Antigravity. Backend: {message}"),
        503 if message.contains("No capacity") => {
            "This model has no capacity right now. Try another model.".to_string()
        }
        other => format!("Antigravity API error ({other}): {message}"),
    }
}

pub(super) fn friendly_error(status: Option<u16>, body: &str) -> String {
    let message = parse_error_message(body);
    match status {
        Some(code) => format_status_error(code, &message),
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
