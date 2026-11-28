//! Shared Antigravity HTTP plumbing: Cloud Code Assist endpoints, request
//! headers, error mapping, and the metadata POST helper.

use crate::http::{CLIENT, bearer, header_value};
use crate::provider::error::{HttpFailure, error_message_or_excerpt};
use reqwest::header::{HeaderMap, HeaderValue};
use std::time::Duration;

pub const DEFAULT_ENDPOINT: &str = "https://daily-cloudcode-pa.googleapis.com";
pub const ENDPOINT_CANDIDATES: [&str; 3] = [
    DEFAULT_ENDPOINT,
    "https://cloudcode-pa.googleapis.com",
    "https://daily-cloudcode-pa.sandbox.googleapis.com",
];

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);

/// Headers Cloud Code Assist expects on every call (pi-antigravity parity).
pub fn antigravity_headers(token: &str) -> Result<HeaderMap, HttpFailure> {
    let platform = match std::env::consts::OS {
        "macos" => "MACOS",
        "windows" => "WINDOWS",
        _ => "LINUX",
    };
    let mut headers = HeaderMap::new();
    headers.insert("Authorization", bearer(token)?);
    headers.insert("Content-Type", HeaderValue::from_static("application/json"));
    headers.insert(
        "User-Agent",
        HeaderValue::from_static("antigravity/hub/2.8.0 (aidev_client; os_type=darwin; arch=arm64; cl=963137146)"),
    );
    headers.insert(
        "X-Goog-Api-Client",
        HeaderValue::from_static("google-cloud-sdk vscode_cloudshelleditor/0.1"),
    );
    headers.insert(
        "Client-Metadata",
        header_value(
            &format!(r#"{{"ideType":"ANTIGRAVITY","platform":"{platform}","pluginType":"GEMINI"}}"#),
            "The client metadata",
        )?,
    );
    Ok(headers)
}

fn extract_field_violations(details: &[serde_json::Value]) -> Vec<String> {
    let mut violations = Vec::new();
    for item in details {
        let Some(fvs) = item.get("fieldViolations").and_then(|f| f.as_array()) else {
            continue;
        };
        for fv in fvs {
            let field = fv.get("field").and_then(|f| f.as_str()).unwrap_or("");
            let desc = fv.get("description").and_then(|d| d.as_str()).unwrap_or("");
            if !field.is_empty() || !desc.is_empty() {
                violations.push(format!("{field}: {desc}"));
            }
        }
    }
    violations
}

fn format_error_details(message: &str, details: &[serde_json::Value]) -> String {
    let violations = extract_field_violations(details);
    if !violations.is_empty() {
        format!("{message} (details: {})", violations.join("; "))
    } else {
        format!("{message} (raw details: {details:?})")
    }
}

fn parse_error_message(body: &str) -> String {
    let message = error_message_or_excerpt(body);
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return message;
    };
    let Some(err) = v.get("error") else {
        return message;
    };
    match err.get("details").and_then(|d| d.as_array()) {
        Some(details) => format_error_details(&message, details),
        None => format!("{message} (body: {v})"),
    }
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
        None => {
            let clean = message.strip_prefix("Antigravity request failed: ").unwrap_or(&message);
            format!("Antigravity request failed: {clean}")
        }
    }
}

pub(crate) fn resolve_endpoints_with(
    explicit: Option<&str>,
    mut env_lookup: impl FnMut(&str) -> Option<String>,
) -> Vec<String> {
    if let Some(endpoint) = explicit {
        return vec![endpoint.to_string()];
    }
    for var in ["ANTIGRAVITY_ENDPOINT", "ANTIGRAVITY_BASE_URL"] {
        if let Some(val) = env_lookup(var) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return vec![trimmed.to_string()];
            }
        }
    }
    ENDPOINT_CANDIDATES.iter().map(|&s| s.to_string()).collect()
}

pub fn resolve_endpoints(explicit: Option<&str>) -> Vec<String> {
    resolve_endpoints_with(explicit, |var| std::env::var(var).ok())
}

pub(crate) async fn post_metadata_candidates(
    endpoints: &[String],
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> Option<serde_json::Value> {
    for endpoint in endpoints {
        let Ok(headers) = antigravity_headers(token) else {
            return None;
        };
        let response = CLIENT
            .post(format!("{endpoint}{path}"))
            .headers(headers)
            .json(&body)
            .timeout(DISCOVERY_TIMEOUT)
            .send()
            .await;
        if let Ok(res) = response
            && res.status().is_success()
            && let Ok(json) = res.json::<serde_json::Value>().await
        {
            return Some(json);
        }
    }
    None
}
