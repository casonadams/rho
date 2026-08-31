use super::types::{QuotaWindow, parse_reset_time};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct CodexRateLimitWindow {
    pub percent_left: Option<f64>,
    pub remaining_percent: Option<f64>,
    pub used_percent: Option<f64>,
    pub reset_at: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct CodexUsageResponse {
    pub rate_limit: Option<CodexRateLimits>,
    pub rate_limits: Option<CodexRateLimits>,
}

#[derive(Debug, Deserialize)]
pub struct CodexRateLimits {
    pub primary_window: Option<CodexRateLimitWindow>,
    pub five_hour: Option<CodexRateLimitWindow>,
    pub secondary_window: Option<CodexRateLimitWindow>,
    pub weekly: Option<CodexRateLimitWindow>,
}

pub fn parse_codex_usage(data: &CodexUsageResponse) -> Vec<QuotaWindow> {
    let limits = data.rate_limit.as_ref().or(data.rate_limits.as_ref());
    let mut windows = Vec::new();
    if let Some(limits) = limits {
        if let Some(secondary) = limits.secondary_window.as_ref().or(limits.weekly.as_ref()) {
            let used = secondary
                .used_percent
                .or_else(|| secondary.remaining_percent.map(|r| 100.0 - r))
                .or_else(|| secondary.percent_left.map(|r| 100.0 - r))
                .unwrap_or(0.0);
            windows.push(QuotaWindow {
                label: "weekly".to_string(),
                used_percent: used,
                resets_at: parse_reset_time(&secondary.reset_at),
                used_value: used,
                limit_value: 100.0,
                is_currency: false,
                limited: false,
            });
        }

        if let Some(primary) = limits.primary_window.as_ref().or(limits.five_hour.as_ref()) {
            let used = primary
                .used_percent
                .or_else(|| primary.remaining_percent.map(|r| 100.0 - r))
                .or_else(|| primary.percent_left.map(|r| 100.0 - r))
                .unwrap_or(0.0);
            windows.push(QuotaWindow {
                label: "session".to_string(),
                used_percent: used,
                resets_at: parse_reset_time(&primary.reset_at),
                used_value: used,
                limit_value: 100.0,
                is_currency: false,
                limited: false,
            });
        }
    }
    windows
}

use std::path::Path;

#[derive(Debug, Deserialize)]
struct ChatGptAuthFile {
    access_token: Option<String>,
    account_id: Option<String>,
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
    super::types::format_quota_windows(&windows)
}
