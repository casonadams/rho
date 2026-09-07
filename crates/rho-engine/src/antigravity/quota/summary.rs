//! Grouped quota summary parsing for Antigravity retrieveUserQuotaSummary.

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{combine_windows, format_quota_window};

pub fn parse_quota_summary(value: &Value, target_model: &str, now: DateTime<Utc>) -> Option<String> {
    let buckets = extract_summary_buckets(value, target_model)?;
    let (five_hour, weekly) = classify_buckets(buckets, now);
    combine_windows(five_hour, weekly)
}

fn extract_summary_buckets<'a>(value: &'a Value, target_model: &str) -> Option<&'a Vec<Value>> {
    let groups = value
        .get("groups")
        .or_else(|| value.get("userQuota").and_then(|u| u.get("groups")))
        .or_else(|| value.get("quota").and_then(|q| q.get("groups")));

    if let Some(groups) = groups.and_then(|g| g.as_array()) {
        select_group_buckets(groups, target_model)
    } else {
        value.get("buckets").and_then(|b| b.as_array())
    }
}

fn classify_buckets(buckets: &[Value], now: DateTime<Utc>) -> (Option<String>, Option<String>) {
    let mut five_hour = None;
    let mut weekly = None;
    for bucket in buckets {
        let Some((formatted, is_week, is_5h)) = inspect_bucket(bucket, now) else {
            continue;
        };
        if is_week {
            weekly = Some(formatted);
        } else if is_5h {
            five_hour = Some(formatted);
        }
    }
    (five_hour, weekly)
}

fn inspect_bucket(bucket: &Value, now: DateTime<Utc>) -> Option<(String, bool, bool)> {
    let fraction = bucket
        .get("remainingFraction")
        .or_else(|| bucket.get("fraction"))
        .and_then(|v| v.as_f64())?;
    let reset_time = bucket
        .get("resetTime")
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));
    let formatted = format_quota_window(fraction, reset_time, now);
    let is_week = is_weekly_bucket(bucket, reset_time, now);
    let is_5h = is_5h_bucket(bucket, reset_time, now);
    Some((formatted, is_week, is_5h))
}

fn group_matches_target(group: &Value, target: &str) -> bool {
    let name = group
        .get("displayName")
        .or_else(|| group.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let buckets_text: String = group
        .get("buckets")
        .and_then(|b| b.as_array())
        .map(|arr| arr.iter().map(extract_bucket_text).collect::<Vec<_>>().join(" "))
        .unwrap_or_default();

    if target.starts_with("gemini") {
        name.contains("gemini") || buckets_text.contains("gemini")
    } else if target.starts_with("claude") {
        name.contains("claude")
            || name.contains("3p")
            || name.contains("third")
            || buckets_text.contains("claude")
            || buckets_text.contains("3p")
    } else if target.starts_with("gpt") {
        name.contains("gpt") || name.contains("claude") || name.contains("3p") || name.contains("other")
    } else {
        name.contains(target)
    }
}

fn select_group_buckets<'a>(groups: &'a [Value], target_model: &str) -> Option<&'a Vec<Value>> {
    let target = target_model.trim().to_ascii_lowercase();
    if let Some(group) = groups.iter().find(|g| group_matches_target(g, &target))
        && let Some(buckets) = group.get("buckets").and_then(|b| b.as_array())
        && !buckets.is_empty()
    {
        return Some(buckets);
    }
    groups
        .iter()
        .find_map(|g| g.get("buckets").and_then(|b| b.as_array()).filter(|b| !b.is_empty()))
}

fn extract_bucket_text(bucket: &Value) -> String {
    let mut s = String::new();
    for key in ["displayName", "window", "bucketId", "description", "label", "name"] {
        if let Some(v) = bucket.get(key).and_then(|v| v.as_str()) {
            s.push(' ');
            s.push_str(v);
        }
    }
    s.to_ascii_lowercase()
}

fn is_weekly_bucket(bucket: &Value, reset_time: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    let text = extract_bucket_text(bucket);
    if text.contains("week") || text.contains("7d") || text.contains("wk") || text.contains("168h") {
        return true;
    }
    reset_time.is_some_and(|reset| reset.signed_duration_since(now).num_hours() > 36)
}

fn is_5h_bucket(bucket: &Value, reset_time: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    let text = extract_bucket_text(bucket);
    if text.contains("5h")
        || text.contains("5 hour")
        || text.contains("5hour")
        || text.contains("five")
        || text.contains("session")
        || text.contains("300m")
    {
        return true;
    }
    reset_time.is_some_and(|reset| {
        let hours = reset.signed_duration_since(now).num_hours();
        hours > 0 && hours <= 36
    })
}
