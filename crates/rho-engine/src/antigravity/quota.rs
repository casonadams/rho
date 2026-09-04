//! Antigravity rolling quota fetching, parsing, and countdown formatting.

use chrono::{DateTime, Duration, Utc};
use serde_json::Value;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq)]
pub struct ModelQuota {
    pub model_id: String,
    pub remaining_fraction: f64,
    pub reset_time: Option<DateTime<Utc>>,
}

/// Fetch available models from Antigravity and extract active quota display.
pub async fn fetch_quota(token: &str, project_id: &str, target_model: &str) -> Option<String> {
    let response = super::client::post_metadata(
        "/v1internal:fetchAvailableModels",
        token,
        serde_json::json!({ "project": project_id }),
    )
    .await?;
    parse_quota(&response, target_model, Utc::now())
}

/// Parse `quotaInfo` from `fetchAvailableModels` JSON and format the status string.
pub fn parse_quota(value: &Value, target_model: &str, now: DateTime<Utc>) -> Option<String> {
    let models_obj = value.get("models")?.as_object()?;
    let mut candidates = Vec::new();

    for (model_id, info) in models_obj {
        let Some(quota_info) = info.get("quotaInfo") else {
            continue;
        };
        let Some(remaining) = quota_info.get("remainingFraction").and_then(|v| v.as_f64()) else {
            continue;
        };
        let reset_time = quota_info
            .get("resetTime")
            .and_then(|v| v.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        candidates.push(ModelQuota {
            model_id: model_id.clone(),
            remaining_fraction: remaining,
            reset_time,
        });
    }

    if candidates.is_empty() {
        return None;
    }

    let selected = select_model_quota(&candidates, target_model)?;
    Some(format_model_quota(selected, now))
}

fn select_model_quota<'a>(candidates: &'a [ModelQuota], target: &str) -> Option<&'a ModelQuota> {
    let target_clean = target.trim().to_ascii_lowercase();

    // 1. Exact match (case-insensitive)
    if let Some(exact) = candidates
        .iter()
        .find(|c| c.model_id.eq_ignore_ascii_case(&target_clean))
    {
        return Some(exact);
    }

    // 2. Prefix / substring match with lowest remaining fraction
    let prefix_matches: Vec<&ModelQuota> = candidates
        .iter()
        .filter(|c| {
            let id = c.model_id.to_ascii_lowercase();
            id.starts_with(&target_clean) || target_clean.starts_with(&id)
        })
        .collect();

    if let Some(lowest) = prefix_matches
        .into_iter()
        .min_by(|a, b| a.remaining_fraction.total_cmp(&b.remaining_fraction))
    {
        return Some(lowest);
    }

    // 3. Fallback: candidate with lowest remaining fraction across all
    candidates
        .iter()
        .min_by(|a, b| a.remaining_fraction.total_cmp(&b.remaining_fraction))
}

fn format_model_quota(quota: &ModelQuota, now: DateTime<Utc>) -> String {
    let pct = (quota.remaining_fraction * 100.0).round().clamp(0.0, 100.0) as u64;
    let Some(reset_time) = quota.reset_time else {
        return format!("{pct}%");
    };

    let duration = reset_time.signed_duration_since(now);
    if duration.num_seconds() <= 0 {
        return format!("{pct}%");
    }

    let countdown = format_duration(duration);
    format!("{pct}% ({countdown})")
}

fn format_duration(duration: Duration) -> String {
    let total_secs = duration.num_seconds().max(1);
    let days = duration.num_days();
    let hours = duration.num_hours();
    let minutes = duration.num_minutes();

    if days >= 1 {
        let rem_hours = (hours % 24).max(0);
        format!("{days}d{rem_hours}h")
    } else if hours >= 1 {
        let rem_mins = (minutes % 60).max(0);
        format!("{hours}h{rem_mins}m")
    } else if minutes >= 1 {
        format!("{minutes}m")
    } else {
        format!("{total_secs}s")
    }
}
