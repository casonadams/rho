//! Claude OAuth usage and rate limit quota display.

#[cfg(test)]
mod tests;

use crate::http::{CLIENT, send_json_if_ok};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::time::Duration;

pub const USAGE_ENDPOINT: &str = "https://api.anthropic.com/api/oauth/usage";

pub async fn fetch_quota(token: &str, target_model: Option<&str>) -> Option<String> {
    let request = CLIENT
        .get(USAGE_ENDPOINT)
        .timeout(Duration::from_secs(8))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", crate::auth::claude::USER_AGENT);
    let response = send_json_if_ok(request).await?;
    parse_quota(&response, target_model, Utc::now())
}

pub fn parse_quota(value: &Value, target_model: Option<&str>, now: DateTime<Utc>) -> Option<String> {
    let five_hour = value.get("five_hour").and_then(|w| format_window(w, now));

    let is_opus = target_model.is_some_and(|m| m.to_ascii_lowercase().contains("opus"));
    let is_sonnet = target_model.is_some_and(|m| m.to_ascii_lowercase().contains("sonnet"));

    let secondary = if is_opus && value.get("seven_day_opus").is_some_and(|v| !v.is_null()) {
        value.get("seven_day_opus")
    } else if is_sonnet && value.get("seven_day_sonnet").is_some_and(|v| !v.is_null()) {
        value.get("seven_day_sonnet")
    } else {
        value.get("seven_day")
    };

    let weekly = secondary.and_then(|w| format_window(w, now));

    crate::quota::combine_windows(five_hour, weekly)
}

fn format_window(window: &Value, now: DateTime<Utc>) -> Option<String> {
    crate::quota::format_window(window, &WINDOW, now)
}

const WINDOW: crate::quota::WindowSpec = crate::quota::WindowSpec {
    percent: &[
        crate::quota::Percent::Used("utilization"),
        crate::quota::Percent::Remaining("remaining_percent"),
        crate::quota::Percent::Remaining("percent_left"),
    ],
    reset: &["resets_at", "reset_at"],
};
