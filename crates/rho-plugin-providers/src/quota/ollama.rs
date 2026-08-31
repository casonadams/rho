use super::types::QuotaWindow;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ChatGptAuthFile {
    pub access_token: Option<String>,
    pub account_id: Option<String>,
    pub usage: Option<CodexUsageResponseWrapper>,
}

#[derive(Debug, Deserialize)]
pub struct CodexUsageResponseWrapper {
    pub rate_limit: Option<super::codex::CodexRateLimits>,
    pub rate_limits: Option<super::codex::CodexRateLimits>,
}

#[derive(Debug, Deserialize)]
pub struct OllamaAuthFile {
    pub key: Option<String>,
    pub usage: Option<OllamaUsageResponse>,
}

#[derive(Debug, Deserialize)]
pub struct OllamaUsageResponse {
    pub limits: Option<OllamaLimits>,
}

#[derive(Debug, Deserialize)]
pub struct OllamaLimits {
    pub session: Option<OllamaUsageLimit>,
    pub weekly: Option<OllamaUsageLimit>,
}

#[derive(Debug, Deserialize)]
pub struct OllamaUsageLimit {
    pub usage: Option<f64>,
}

pub fn is_ollama_cloud_model(model_id: &str) -> bool {
    model_id.ends_with(":cloud")
}

// The /api/usage endpoint is undocumented and may change or disappear without
// notice. It exposes no quota reset timestamps, so windows render without
// countdowns.
pub fn parse_ollama_usage(body: &OllamaUsageResponse) -> Vec<QuotaWindow> {
    let Some(limits) = body.limits.as_ref() else {
        return Vec::new();
    };
    let (Some(session), Some(weekly)) = (limits.session.as_ref(), limits.weekly.as_ref()) else {
        return Vec::new();
    };
    let mut windows = Vec::new();
    for (label, limit) in [("7d", weekly), ("5h", session)] {
        let Some(usage) = limit.usage else {
            continue;
        };
        if !usage.is_finite() {
            continue;
        }
        let used_percent = usage.clamp(0.0, 1.0) * 100.0;
        windows.push(QuotaWindow {
            label: label.to_string(),
            used_percent,
            resets_at: None,
            used_value: used_percent,
            limit_value: 100.0,
            is_currency: false,
            limited: false,
        });
    }
    windows
}
