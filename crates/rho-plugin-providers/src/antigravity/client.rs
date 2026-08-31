use reqwest::header::{HeaderMap, HeaderValue};
use rho_core::error::{AppError, Result};
use sha2::{Digest, Sha256};
use std::sync::LazyLock;

pub static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .no_proxy()
        .use_rustls_tls()
        .build()
        .expect("Failed to initialize Antigravity HTTP client")
});

pub const DEFAULT_ENDPOINT: &str = "https://daily-cloudcode-pa.googleapis.com";
pub const ENDPOINT_FALLBACKS: &[&str] = &[
    "https://daily-cloudcode-pa.googleapis.com",
    "https://daily-cloudcode-pa.sandbox.googleapis.com",
    "https://cloudcode-pa.googleapis.com",
];

pub fn endpoint_candidates() -> Vec<String> {
    if let Ok(explicit) = std::env::var("ANTIGRAVITY_BASE_URL")
        && !explicit.trim().is_empty()
    {
        return vec![explicit.trim().to_string()];
    }
    ENDPOINT_FALLBACKS.iter().map(|s| s.to_string()).collect()
}

pub fn stable_project_id(seed: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("antigravity:{seed}").as_bytes());
    let bytes = hasher.finalize();
    let hex = bytes[..16].iter().map(|b| format!("{b:02x}")).collect::<String>();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

pub fn default_project_id(email: Option<&str>) -> String {
    if let Ok(id) = std::env::var("ANTIGRAVITY_PROJECT_ID")
        && !id.trim().is_empty()
    {
        return id.trim().to_string();
    }
    stable_project_id(email.unwrap_or("antigravity-default"))
}

pub fn antigravity_headers(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let auth = format!("Bearer {token}");
    if let Ok(val) = HeaderValue::from_str(&auth) {
        headers.insert(reqwest::header::AUTHORIZATION, val);
    }
    headers.insert(
        reqwest::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(reqwest::header::ACCEPT, HeaderValue::from_static("text/event-stream"));
    headers.insert(
        reqwest::header::USER_AGENT,
        HeaderValue::from_static("antigravity/hub/2.8.0 (aidev_client; os_type=darwin; arch=arm64; cl=963137146)"),
    );
    headers.insert(
        "X-Goog-Api-Client",
        HeaderValue::from_static("google-cloud-sdk vscode_cloudshelleditor/0.1"),
    );
    headers.insert(
        "Client-Metadata",
        HeaderValue::from_static("{\"ideType\":\"ANTIGRAVITY\",\"platform\":\"MACOS\",\"pluginType\":\"GEMINI\"}"),
    );
    headers
}

pub fn antigravity_json_headers(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let auth = format!("Bearer {token}");
    if let Ok(val) = HeaderValue::from_str(&auth) {
        headers.insert(reqwest::header::AUTHORIZATION, val);
    }
    headers.insert(
        reqwest::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(reqwest::header::ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(
        reqwest::header::USER_AGENT,
        HeaderValue::from_static("antigravity/hub/2.8.0 (aidev_client; os_type=darwin; arch=arm64; cl=963137146)"),
    );
    headers.insert(
        "X-Goog-Api-Client",
        HeaderValue::from_static("google-cloud-sdk vscode_cloudshelleditor/0.1"),
    );
    headers.insert(
        "Client-Metadata",
        HeaderValue::from_static("{\"ideType\":\"ANTIGRAVITY\",\"platform\":\"MACOS\",\"pluginType\":\"GEMINI\"}"),
    );
    headers
}

pub async fn discover_project_id(access_token: &str) -> Option<String> {
    for endpoint in endpoint_candidates() {
        let url = format!("{endpoint}/v1internal:loadCodeAssist");
        let headers = antigravity_json_headers(access_token);
        let res = HTTP_CLIENT
            .post(&url)
            .headers(headers)
            .json(&serde_json::json!({}))
            .send()
            .await;
        if let Ok(response) = res
            && response.status().is_success()
            && let Ok(val) = response.json::<serde_json::Value>().await
        {
            if let Some(id) = val.get("cloudaicompanionProject").and_then(|v| v.as_str()) {
                return Some(id.to_string());
            }
            if let Some(id) = val.get("projectId").and_then(|v| v.as_str()) {
                return Some(id.to_string());
            }
        }
    }
    None
}

pub async fn fetch_account_usage(access_token: &str, project_id: Option<&str>) -> Result<serde_json::Value> {
    let body = match project_id {
        Some(p) => serde_json::json!({ "project": p }),
        None => serde_json::json!({}),
    };
    for endpoint in endpoint_candidates() {
        let headers = antigravity_json_headers(access_token);

        let models_url = format!("{endpoint}/v1internal:fetchAvailableModels");
        if let Ok(res) = HTTP_CLIENT
            .post(&models_url)
            .headers(headers.clone())
            .json(&body)
            .send()
            .await
            && res.status().is_success()
            && let Ok(val) = res.json::<serde_json::Value>().await
        {
            return Ok(val);
        }

        let summary_url = format!("{endpoint}/v1internal:retrieveUserQuotaSummary");
        if let Ok(res) = HTTP_CLIENT.post(&summary_url).headers(headers).json(&body).send().await
            && res.status().is_success()
            && let Ok(val) = res.json::<serde_json::Value>().await
        {
            return Ok(val);
        }
    }
    Err(AppError::Provider(
        "Failed to fetch Antigravity account usage".to_string(),
    ))
}

pub fn runtime_candidates(model: &str) -> Vec<String> {
    match model {
        "gemini-3.7-flash" => vec![
            "gemini-3.7-flash-high".to_string(),
            "gemini-3.7-flash-low".to_string(),
            "gemini-3.6-flash-high".to_string(),
            "gemini-3.7-flash".to_string(),
        ],
        "gemini-3.6-flash" => vec![
            "gemini-3.6-flash-high".to_string(),
            "gemini-3.6-flash-low".to_string(),
            "gemini-3.6-flash".to_string(),
        ],
        "gemini-3.5-flash" => vec![
            "gemini-3-flash-agent".to_string(),
            "gemini-3.5-flash-low".to_string(),
            "gemini-3.5-flash-extra-low".to_string(),
        ],
        "gemini-3.1-pro" => vec!["gemini-pro-agent".to_string(), "gemini-3.1-pro-low".to_string()],
        "claude-sonnet-4-6" => vec!["claude-sonnet-4-6".to_string()],
        "claude-opus-4-6" => vec!["claude-opus-4-6-thinking".to_string()],
        "gpt-oss-120b" => vec!["gpt-oss-120b-medium".to_string()],
        other => vec![other.to_string()],
    }
}

pub fn map_runtime_model(model: &str) -> String {
    match model {
        "gemini-3.7-flash" => "gemini-3.7-flash-high".to_string(),
        "gemini-3.6-flash" => "gemini-3.6-flash-high".to_string(),
        "gemini-3.5-flash" => "gemini-3.5-flash-low".to_string(),
        "gemini-3.1-pro" => "gemini-pro-agent".to_string(),
        "claude-sonnet-4-6" => "claude-sonnet-4-6".to_string(),
        "claude-opus-4-6" => "claude-opus-4-6-thinking".to_string(),
        "gpt-oss-120b" => "gpt-oss-120b-medium".to_string(),
        other => other.to_string(),
    }
}
