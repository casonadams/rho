pub mod antigravity;
pub mod codex;
pub mod ollama;
#[cfg(test)]
mod tests;
pub mod types;

pub use antigravity::parse_antigravity_usage;
pub use codex::{CodexRateLimitWindow, CodexRateLimits, CodexUsageResponse, parse_codex_usage};
pub use ollama::{
    ChatGptAuthFile, CodexUsageResponseWrapper, OllamaAuthFile, OllamaLimits, OllamaUsageLimit, OllamaUsageResponse,
    parse_ollama_usage,
};
pub use types::{QuotaWindow, format_quota_windows};

use std::path::Path;

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

pub async fn fetch_chatgpt_quota(config_dir: &Path) -> Option<String> {
    let auth_path = config_dir.join("tokens/chatgpt/auth.json");
    if !auth_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(auth_path).ok()?;
    let auth_data: ChatGptAuthFile = serde_json::from_str(&content).ok()?;
    let access_token = auth_data.access_token?;
    let account_id = auth_data.account_id?;

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(4))
        .build()
        .ok()?;

    let res = client
        .get("https://chatgpt.com/backend-api/wham/usage")
        .header("Authorization", format!("Bearer {access_token}"))
        .header("ChatGPT-Account-Id", account_id)
        .header("Accept", "application/json")
        .header("Origin", "https://chatgpt.com")
        .header("Referer", "https://chatgpt.com/")
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .ok()?;

    if !res.status().is_success() {
        return None;
    }

    let body: CodexUsageResponse = res.json().await.ok()?;
    let windows = parse_codex_usage(&body);
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

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(4))
        .build()
        .ok()?;

    let res = client
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
