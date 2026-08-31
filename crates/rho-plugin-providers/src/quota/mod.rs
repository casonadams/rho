pub mod antigravity;
pub mod codex;
pub mod ollama;
#[cfg(test)]
mod tests;
pub mod types;

pub use antigravity::parse_antigravity_usage;
pub use codex::{CodexRateLimitWindow, CodexRateLimits, CodexUsageResponse, fetch_chatgpt_quota, parse_codex_usage};
pub use ollama::{
    ChatGptAuthFile, CodexUsageResponseWrapper, OllamaAuthFile, OllamaLimits, OllamaUsageLimit, OllamaUsageResponse,
    parse_ollama_usage,
};
pub use types::{QuotaWindow, format_quota_windows};

use std::path::Path;
use std::sync::LazyLock;

pub(crate) static QUOTA_HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(4))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
});

pub fn is_ollama_cloud_model(model_id: &str) -> bool {
    model_id.ends_with(":cloud")
}

pub async fn fetch_antigravity_quota(config_dir: &Path, model_id: &str) -> Option<String> {
    let provider = crate::antigravity::AntigravityProvider::new(config_dir.to_path_buf());
    let tokens = provider.ensure_valid_tokens().await.ok()?;
    let usage = crate::antigravity::fetch_account_usage(&tokens.access_token, tokens.project_id.as_deref())
        .await
        .ok()?;
    let windows = parse_antigravity_usage(&usage, Some(model_id));
    format_quota_windows(&windows)
}

pub async fn fetch_ollama_cloud_quota(config_dir: &Path, model_id: &str) -> Option<String> {
    if !is_ollama_cloud_model(model_id) {
        return None;
    }
    let auth_path = config_dir.join("tokens/ollama-cloud/auth.json");
    if !auth_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(auth_path).ok()?;
    let auth_data: OllamaAuthFile = serde_json::from_str(&content).ok()?;
    let key = auth_data.key?;

    let res = QUOTA_HTTP_CLIENT
        .get("https://ollama.com/api/usage")
        .header("Authorization", format!("Bearer {key}"))
        .header("Accept", "application/json")
        .send()
        .await
        .ok()?;

    if !res.status().is_success() {
        return None;
    }

    let body: OllamaUsageResponse = res.json().await.ok()?;
    let windows = parse_ollama_usage(&body);
    format_quota_windows(&windows)
}
