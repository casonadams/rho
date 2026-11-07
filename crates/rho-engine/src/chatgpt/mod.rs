//! OpenAI ChatGPT (Codex) account quota display.

#[cfg(test)]
mod tests;

pub mod client;
pub mod stream;

pub use client::{ChatGptClient, into_handle};

use crate::http::{CLIENT, send_json_if_ok};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::time::Duration;

pub const USAGE_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";

/// Fetch account usage from chatgpt.com backend and format the status string.
pub async fn fetch_quota(token: &str, account_id: Option<&str>) -> Option<String> {
    let mut req = CLIENT
        .get(USAGE_ENDPOINT)
        .timeout(Duration::from_secs(8))
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json")
        .header("Origin", "https://chatgpt.com")
        .header("Referer", "https://chatgpt.com/")
        .header("User-Agent", "Mozilla/5.0");

    if let Some(account_id) = account_id.filter(|s| !s.is_empty()) {
        req = req.header("ChatGPT-Account-Id", account_id);
    }

    let response = send_json_if_ok(req).await?;
    parse_quota(&response, Utc::now())
}

/// Parse rate limits from `wham/usage` JSON and format 5h then weekly display.
pub fn parse_quota(value: &Value, now: DateTime<Utc>) -> Option<String> {
    let limits = value.get("rate_limit").or_else(|| value.get("rate_limits"))?;

    let primary = limits.get("primary_window").or_else(|| limits.get("five_hour"));
    let secondary = limits.get("secondary_window").or_else(|| limits.get("weekly"));

    let five_hour = primary.and_then(|w| format_window(w, now));
    let weekly = secondary.and_then(|w| format_window(w, now));

    crate::quota::combine_windows(five_hour, weekly)
}

fn format_window(window: &Value, now: DateTime<Utc>) -> Option<String> {
    crate::quota::format_window(window, &WINDOW, now)
}

const WINDOW: crate::quota::WindowSpec = crate::quota::WindowSpec {
    percent: &[
        crate::quota::Percent::Remaining("remaining_percent"),
        crate::quota::Percent::Remaining("percent_left"),
        crate::quota::Percent::Used("used_percent"),
    ],
    reset: &["reset_at"],
};
