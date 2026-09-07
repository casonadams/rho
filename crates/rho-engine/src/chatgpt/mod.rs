//! OpenAI ChatGPT (Codex) account quota display.

#[cfg(test)]
mod tests;

use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::LazyLock;
use std::time::Duration;

pub const USAGE_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    crate::install_crypto_provider();
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(8))
        .build()
        .unwrap_or_default()
});

/// Fetch account usage from chatgpt.com backend and format the status string.
pub async fn fetch_quota(token: &str, account_id: Option<&str>) -> Option<String> {
    let mut req = CLIENT
        .get(USAGE_ENDPOINT)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json")
        .header("Origin", "https://chatgpt.com")
        .header("Referer", "https://chatgpt.com/")
        .header("User-Agent", "Mozilla/5.0");

    if let Some(account_id) = account_id.filter(|s| !s.is_empty()) {
        req = req.header("ChatGPT-Account-Id", account_id);
    }

    let response = req.send().await.ok()?.json::<Value>().await.ok()?;
    parse_quota(&response, Utc::now())
}

/// Parse rate limits from `wham/usage` JSON and format 5h then weekly display.
pub fn parse_quota(value: &Value, now: DateTime<Utc>) -> Option<String> {
    let limits = value.get("rate_limit").or_else(|| value.get("rate_limits"))?;

    let primary = limits.get("primary_window").or_else(|| limits.get("five_hour"));
    let secondary = limits.get("secondary_window").or_else(|| limits.get("weekly"));

    let five_hour = primary.and_then(|w| format_window(w, now));
    let weekly = secondary.and_then(|w| format_window(w, now));

    crate::antigravity::quota::combine_windows(five_hour, weekly)
}

fn format_window(window: &Value, now: DateTime<Utc>) -> Option<String> {
    let fraction = extract_remaining_fraction(window)?;
    let reset_time = parse_reset_time(window.get("reset_at"));
    Some(crate::antigravity::quota::format_quota_window(
        fraction, reset_time, now,
    ))
}

fn extract_remaining_fraction(window: &Value) -> Option<f64> {
    if let Some(r) = window.get("remaining_percent").and_then(|v| v.as_f64()) {
        return Some(r / 100.0);
    }
    if let Some(p) = window.get("percent_left").and_then(|v| v.as_f64()) {
        return Some(p / 100.0);
    }
    if let Some(u) = window.get("used_percent").and_then(|v| v.as_f64()) {
        return Some((100.0 - u) / 100.0);
    }
    None
}

fn parse_reset_time(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let val = value?;
    if let Some(ts) = val.as_i64() {
        DateTime::from_timestamp(ts, 0)
    } else if let Some(ts_f) = val.as_f64() {
        DateTime::from_timestamp(ts_f as i64, 0)
    } else if let Some(s) = val.as_str() {
        DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&Utc))
    } else {
        None
    }
}
