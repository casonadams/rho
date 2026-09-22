//! Grouped quota summary parsing for Antigravity retrieveUserQuotaSummary.

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{combine_windows, format_quota_window};

pub fn parse_quota_summary(value: &Value, target_model: &str, now: DateTime<Utc>) -> Option<String> {
    let buckets = extract_summary_buckets(value, target_model)?;
    let (five_hour, weekly) = classify_buckets(buckets, target_model, now);
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

fn classify_buckets(buckets: &[Value], target_model: &str, now: DateTime<Utc>) -> (Option<String>, Option<String>) {
    let matching: Vec<&Value> = buckets
        .iter()
        .filter(|b| bucket_matches_target(b, target_model))
        .collect();

    let candidates: &[&Value] = if matching.is_empty() {
        &buckets.iter().collect::<Vec<_>>()
    } else {
        &matching
    };

    let mut five_hour: Option<(String, f64)> = None;
    let mut weekly: Option<(String, f64)> = None;

    for bucket in candidates {
        let Some((formatted, fraction, is_week, is_5h)) = inspect_bucket(bucket, now) else {
            continue;
        };
        if is_week {
            if weekly.as_ref().is_none_or(|(_, prev)| fraction < *prev) {
                weekly = Some((formatted, fraction));
            }
        } else if is_5h && five_hour.as_ref().is_none_or(|(_, prev)| fraction < *prev) {
            five_hour = Some((formatted, fraction));
        }
    }
    (five_hour.map(|(s, _)| s), weekly.map(|(s, _)| s))
}

fn inspect_bucket(bucket: &Value, now: DateTime<Utc>) -> Option<(String, f64, bool, bool)> {
    let fraction = bucket
        .get("remainingFraction")
        .or_else(|| bucket.get("fraction"))
        .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok())))?;
    let reset_time = bucket
        .get("resetTime")
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));
    let formatted = format_quota_window(fraction, reset_time, now);
    let is_week = is_weekly_bucket(bucket, reset_time, now);
    let is_5h = is_5h_bucket(bucket, reset_time, now);
    Some((formatted, fraction, is_week, is_5h))
}

fn canonical_target(target: &str) -> &str {
    let trimmed = target.trim();
    trimmed.strip_prefix("antigravity/").unwrap_or(trimmed)
}

fn bucket_structural_text(bucket: &Value) -> String {
    let mut s = String::new();
    for key in ["bucketId", "id", "name", "window", "displayName", "label"] {
        if let Some(v) = bucket.get(key).and_then(|v| v.as_str()) {
            s.push(' ');
            s.push_str(v);
        }
    }
    s.to_ascii_lowercase()
}

fn bucket_matches_target(bucket: &Value, target: &str) -> bool {
    let text = bucket_structural_text(bucket);
    let target = canonical_target(target).to_ascii_lowercase();

    let has_family_tag = text.contains("gemini")
        || text.contains("claude")
        || text.contains("gpt")
        || text.contains("3p")
        || text.contains("third");

    if !has_family_tag {
        return true;
    }

    if target.starts_with("gemini") {
        text.contains("gemini")
    } else if target.starts_with("claude") {
        text.contains("claude") || text.contains("3p") || text.contains("third")
    } else if target.starts_with("gpt") {
        text.contains("gpt") || text.contains("3p") || text.contains("third") || text.contains("claude")
    } else {
        text.contains(&target)
    }
}

fn extract_group_text(group: &Value) -> String {
    let mut s = String::new();
    for key in ["displayName", "name", "groupId", "id", "description", "label"] {
        if let Some(v) = group.get(key).and_then(|v| v.as_str()) {
            s.push(' ');
            s.push_str(v);
        }
    }
    s.to_ascii_lowercase()
}

const CLAUDE_KEYWORDS: &[&str] = &["claude", "3p", "third"];
const GPT_KEYWORDS: &[&str] = &["gpt", "claude", "3p", "third", "other"];

fn group_matches_target(group: &Value, target: &str) -> bool {
    let group_text = extract_group_text(group);

    let buckets_text: String = group
        .get("buckets")
        .and_then(|b| b.as_array())
        .map(|arr| arr.iter().map(bucket_structural_text).collect::<Vec<_>>().join(" "))
        .unwrap_or_default();

    let matches_any = |keywords: &[&str]| {
        keywords
            .iter()
            .any(|&kw| group_text.contains(kw) || buckets_text.contains(kw))
    };

    let target = canonical_target(target).to_ascii_lowercase();

    if target.starts_with("gemini") {
        matches_any(&["gemini"])
    } else if target.starts_with("claude") {
        matches_any(CLAUDE_KEYWORDS)
    } else if target.starts_with("gpt") {
        matches_any(GPT_KEYWORDS)
    } else {
        group_text.contains(&target) || buckets_text.contains(&target)
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

fn is_weekly_bucket(bucket: &Value, reset_time: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    let text = bucket_structural_text(bucket);
    if text.contains("week") || text.contains("7d") || text.contains("wk") || text.contains("168h") {
        return true;
    }
    reset_time.is_some_and(|reset| reset.signed_duration_since(now).num_hours() > 36)
}

fn is_5h_bucket(bucket: &Value, reset_time: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    let text = bucket_structural_text(bucket);
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
