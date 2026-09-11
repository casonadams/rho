//! Claude OAuth usage and rate limit quota display.

#[cfg(test)]
mod tests;

use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::LazyLock;
use std::time::Duration;

pub const USAGE_ENDPOINT: &str = "https://api.anthropic.com/api/oauth/usage";

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    crate::install_crypto_provider();
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(8))
        .build()
        .unwrap_or_default()
});

pub async fn fetch_quota(token: &str, target_model: Option<&str>) -> Option<String> {
    let response = CLIENT
        .get(USAGE_ENDPOINT)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", super::http::USER_AGENT)
        .send()
        .await
        .ok()?
        .json::<Value>()
        .await
        .ok()?;

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

    crate::antigravity::quota::combine_windows(five_hour, weekly)
}

fn format_window(window: &Value, now: DateTime<Utc>) -> Option<String> {
    if window.is_null() {
        return None;
    }
    let fraction = extract_remaining_fraction(window)?;
    let reset_time = parse_reset_time(window.get("resets_at").or_else(|| window.get("reset_at")));
    Some(crate::antigravity::quota::format_quota_window(
        fraction, reset_time, now,
    ))
}

fn extract_remaining_fraction(window: &Value) -> Option<f64> {
    if let Some(u) = window.get("utilization").and_then(Value::as_f64) {
        return Some((100.0 - u).max(0.0) / 100.0);
    }
    if let Some(r) = window.get("remaining_percent").and_then(Value::as_f64) {
        return Some(r / 100.0);
    }
    if let Some(p) = window.get("percent_left").and_then(Value::as_f64) {
        return Some(p / 100.0);
    }
    None
}

fn parse_reset_time(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let val = value?;
    if let Some(s) = val.as_str() {
        DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&Utc))
    } else if let Some(ts) = val.as_i64() {
        DateTime::from_timestamp(ts, 0)
    } else if let Some(ts_f) = val.as_f64() {
        DateTime::from_timestamp(ts_f as i64, 0)
    } else {
        None
    }
}
