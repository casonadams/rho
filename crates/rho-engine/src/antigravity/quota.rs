//! Antigravity rolling quota fetching, parsing, and countdown formatting.

use crate::quota::format_quota_window;
use chrono::{DateTime, Utc};
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
pub async fn fetch_quota_from_endpoints(
    endpoints: &[String],
    token: &str,
    project_id: &str,
    target_model: &str,
) -> Option<String> {
    let body = project_request_body(project_id);
    let summary_display = try_fetch_summary_from_endpoints(endpoints, token, &body, target_model).await;
    if let Some(ref display) = summary_display
        && !is_unmetered_summary(display)
    {
        return Some(display.clone());
    }

    if let Some(response) =
        super::client::post_metadata_candidates(endpoints, "/v1internal:fetchAvailableModels", token, body).await
        && let Some(models_display) = parse_models_quota(&response, target_model, Utc::now())
        && (!is_unmetered_summary(&models_display) || summary_display.is_none())
    {
        return Some(models_display);
    }

    summary_display
}

/// Fetch available models or quota summary from Antigravity using configured endpoints.
pub async fn fetch_quota(token: &str, project_id: &str, target_model: &str) -> Option<String> {
    fetch_quota_from_endpoints(&super::client::resolve_endpoints(None), token, project_id, target_model).await
}

pub(crate) fn is_unmetered_summary(display: &str) -> bool {
    let tokens: Vec<&str> = display.split_whitespace().collect();
    !tokens.is_empty() && tokens.iter().all(|&t| t == "0%")
}

fn project_request_body(project_id: &str) -> Value {
    if project_id.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::json!({ "project": project_id })
    }
}

async fn try_fetch_summary_from_endpoints(
    endpoints: &[String],
    token: &str,
    body: &Value,
    target: &str,
) -> Option<String> {
    let summary =
        super::client::post_metadata_candidates(endpoints, "/v1internal:retrieveUserQuotaSummary", token, body.clone())
            .await?;
    parse_quota_summary(&summary, target, Utc::now())
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
