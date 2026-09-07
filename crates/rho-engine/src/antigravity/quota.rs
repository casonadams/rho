//! Antigravity rolling quota fetching, parsing, and countdown formatting.

use chrono::{DateTime, Duration, Utc};
use serde_json::Value;

mod summary;
#[cfg(test)]
mod tests;

pub use summary::parse_quota_summary;

#[derive(Debug, Clone, PartialEq)]
pub struct ModelQuota {
    pub model_id: String,
    pub remaining_fraction: f64,
    pub reset_time: Option<DateTime<Utc>>,
}

/// Fetch available models or quota summary from Antigravity and extract active quota display.
pub async fn fetch_quota(token: &str, project_id: &str, target_model: &str) -> Option<String> {
    let body = project_request_body(project_id);
    if let Some(display) = try_fetch_summary(token, &body, target_model).await {
        return Some(display);
    }
    let response = super::client::post_metadata("/v1internal:fetchAvailableModels", token, body).await?;
    parse_quota(&response, target_model, Utc::now())
}

fn project_request_body(project_id: &str) -> Value {
    if project_id.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::json!({ "project": project_id })
    }
}

async fn try_fetch_summary(token: &str, body: &Value, target: &str) -> Option<String> {
    let summary = super::client::post_metadata("/v1internal:retrieveUserQuotaSummary", token, body.clone()).await?;
    parse_quota_summary(&summary, target, Utc::now())
}

/// Parse quota from JSON (either grouped quota summary or per-model catalog) and format status string.
pub fn parse_quota(value: &Value, target_model: &str, now: DateTime<Utc>) -> Option<String> {
    if let Some(summary) = parse_quota_summary(value, target_model, now) {
        return Some(summary);
    }
    parse_models_quota(value, target_model, now)
}

fn extract_model_candidates(models_obj: &serde_json::Map<String, Value>) -> Vec<ModelQuota> {
    models_obj
        .iter()
        .filter_map(|(id, info)| {
            let qi = info.get("quotaInfo")?;
            let remaining = qi.get("remainingFraction")?.as_f64()?;
            let reset_time = qi
                .get("resetTime")
                .and_then(|v| v.as_str())
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc));
            Some(ModelQuota {
                model_id: id.clone(),
                remaining_fraction: remaining,
                reset_time,
            })
        })
        .collect()
}

fn parse_models_quota(value: &Value, target_model: &str, now: DateTime<Utc>) -> Option<String> {
    let models_obj = value.get("models")?.as_object()?;
    let candidates = extract_model_candidates(models_obj);
    let selected = select_model_quota(&candidates, target_model)?;
    Some(format_quota_window(
        selected.remaining_fraction,
        selected.reset_time,
        now,
    ))
}

fn select_model_quota<'a>(candidates: &'a [ModelQuota], target: &str) -> Option<&'a ModelQuota> {
    let target_clean = target.trim().to_ascii_lowercase();

    if let Some(exact) = candidates
        .iter()
        .find(|c| c.model_id.eq_ignore_ascii_case(&target_clean))
    {
        return Some(exact);
    }

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

    candidates
        .iter()
        .min_by(|a, b| a.remaining_fraction.total_cmp(&b.remaining_fraction))
}

pub(crate) fn format_quota_window(
    remaining_fraction: f64,
    reset_time: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> String {
    let pct = (remaining_fraction * 100.0).round().clamp(0.0, 100.0) as u64;
    let Some(reset_time) = reset_time else {
        return format!("{pct}%");
    };

    let duration = reset_time.signed_duration_since(now);
    if duration.num_seconds() <= 0 {
        return format!("{pct}%");
    }

    let countdown = format_duration(duration);
    format!("{pct}% {countdown}")
}

pub(crate) fn combine_windows(five_hour: Option<String>, weekly: Option<String>) -> Option<String> {
    match (five_hour, weekly) {
        (Some(h), Some(w)) => Some(format!("{h} {w}")),
        (Some(h), None) => Some(h),
        (None, Some(w)) => Some(w),
        (None, None) => None,
    }
}

pub(crate) fn format_duration(duration: Duration) -> String {
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
