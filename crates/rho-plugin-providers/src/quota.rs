use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotaWindow {
    pub label: String,
    pub used_percent: f64,
    pub resets_at: Option<DateTime<Utc>>,
    pub used_value: f64,
    pub limit_value: f64,
    pub is_currency: bool,
    pub limited: bool,
}

impl QuotaWindow {
    pub fn remaining_percent(&self) -> f64 {
        (100.0 - self.used_percent).clamp(0.0, 100.0)
    }

    pub fn format_countdown(&self) -> Option<String> {
        self.format_countdown_at(Utc::now())
    }

    pub fn format_countdown_at(&self, now: DateTime<Utc>) -> Option<String> {
        let resets_at = self.resets_at?;
        if resets_at <= now {
            return None;
        }
        let total_seconds = (resets_at - now).num_seconds();
        let total_minutes = (total_seconds + 30) / 60;
        let days = total_minutes / 1_440;
        let hours = (total_minutes % 1_440) / 60;
        let minutes = total_minutes % 60;

        if days > 0 {
            if hours > 0 {
                Some(format!("{days}d{hours}h"))
            } else {
                Some(format!("{days}d"))
            }
        } else if hours > 0 {
            if minutes > 0 {
                Some(format!("{hours}h{minutes}m"))
            } else {
                Some(format!("{hours}h"))
            }
        } else if minutes > 0 {
            Some(format!("{minutes}m"))
        } else {
            Some(format!("{}s", total_seconds % 60))
        }
    }

    pub fn format_summary(&self) -> String {
        let percent = format!("{:.0}%", self.remaining_percent());
        if let Some(countdown) = self.format_countdown() {
            format!("{percent} ({countdown})")
        } else {
            percent
        }
    }
}

pub fn format_quota_windows(windows: &[QuotaWindow]) -> Option<String> {
    if windows.is_empty() {
        return None;
    }
    let parts: Vec<String> = windows.iter().take(2).map(QuotaWindow::format_summary).collect();
    Some(parts.join(" "))
}

#[derive(Debug, Deserialize)]
pub struct CodexRateLimitWindow {
    pub percent_left: Option<f64>,
    pub remaining_percent: Option<f64>,
    pub used_percent: Option<f64>,
    pub reset_at: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct CodexUsageResponse {
    pub rate_limit: Option<CodexRateLimits>,
    pub rate_limits: Option<CodexRateLimits>,
}

#[derive(Debug, Deserialize)]
pub struct CodexRateLimits {
    pub primary_window: Option<CodexRateLimitWindow>,
    pub five_hour: Option<CodexRateLimitWindow>,
    pub secondary_window: Option<CodexRateLimitWindow>,
    pub weekly: Option<CodexRateLimitWindow>,
}

pub fn parse_codex_usage(data: &CodexUsageResponse) -> Vec<QuotaWindow> {
    let limits = data.rate_limit.as_ref().or(data.rate_limits.as_ref());
    let mut windows = Vec::new();
    if let Some(limits) = limits {
        if let Some(primary) = limits.primary_window.as_ref().or(limits.five_hour.as_ref()) {
            let used = primary
                .used_percent
                .or_else(|| primary.percent_left.map(|p| 100.0 - p))
                .or_else(|| primary.remaining_percent.map(|p| 100.0 - p))
                .unwrap_or(0.0);
            windows.push(QuotaWindow {
                label: "5h".to_string(),
                used_percent: used,
                resets_at: parse_reset_time(&primary.reset_at),
                used_value: used,
                limit_value: 100.0,
                is_currency: false,
                limited: false,
            });
        }
        if let Some(secondary) = limits.secondary_window.as_ref().or(limits.weekly.as_ref()) {
            let used = secondary
                .used_percent
                .or_else(|| secondary.percent_left.map(|p| 100.0 - p))
                .or_else(|| secondary.remaining_percent.map(|p| 100.0 - p))
                .unwrap_or(0.0);
            windows.push(QuotaWindow {
                label: "7d".to_string(),
                used_percent: used,
                resets_at: parse_reset_time(&secondary.reset_at),
                used_value: used,
                limit_value: 100.0,
                is_currency: false,
                limited: false,
            });
        }
    }
    windows
}

fn parse_reset_time(value: &Option<serde_json::Value>) -> Option<DateTime<Utc>> {
    let val = value.as_ref()?;
    if let Some(ts) = val.as_i64() {
        DateTime::from_timestamp(ts, 0)
    } else if let Some(s) = val.as_str() {
        s.parse::<DateTime<Utc>>().ok()
    } else {
        None
    }
}

#[derive(Debug, Deserialize)]
struct ChatGptAuthFile {
    access_token: Option<String>,
    account_id: Option<String>,
}

pub async fn fetch_chatgpt_quota(config_dir: &Path) -> Option<String> {
    let auth_path = config_dir.join("tokens/chatgpt/auth.json");
    if !auth_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(auth_path).ok()?;
    let auth_data: ChatGptAuthFile = serde_json::from_str(&content).ok()?;
    let access_token = auth_data.access_token?;
    let account_id = auth_data.account_id?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .build()
        .ok()?;

    let res = client
        .get("https://chatgpt.com/backend-api/wham/usage")
        .header("Authorization", format!("Bearer {access_token}"))
        .header("ChatGPT-Account-Id", account_id)
        .header("Accept", "application/json")
        .header("Origin", "https://chatgpt.com")
        .header("Referer", "https://chatgpt.com/")
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .ok()?;

    if !res.status().is_success() {
        return None;
    }

    let body: CodexUsageResponse = res.json().await.ok()?;
    let windows = parse_codex_usage(&body);
    format_quota_windows(&windows)
}

#[derive(Debug, Deserialize)]
struct OllamaAuthFile {
    key: Option<String>,
}

/// Ollama Cloud usage renders only for local `ollama` models whose id ends with
/// `:cloud`; non-cloud local models consume no cloud quota and stay silent.
pub fn is_ollama_cloud_model(model_id: &str) -> bool {
    model_id.ends_with(":cloud")
}

pub async fn fetch_ollama_cloud_quota(config_dir: &Path, model_id: &str) -> Option<String> {
    if !is_ollama_cloud_model(model_id) {
        return None;
    }
    let auth_path = config_dir.join("tokens/ollama-cloud/auth.json");
    if !auth_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(auth_path).ok()?;
    let auth_data: OllamaAuthFile = serde_json::from_str(&content).ok()?;
    let key = auth_data.key?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .build()
        .ok()?;

    let res = client
        .get("https://ollama.com/api/usage")
        .header("Authorization", format!("Bearer {key}"))
        .header("Accept", "application/json")
        .send()
        .await
        .ok()?;

    if !res.status().is_success() {
        return None;
    }

    let body: OllamaUsageResponse = res.json().await.ok()?;
    let windows = parse_ollama_usage(&body);
    format_quota_windows(&windows)
}

#[derive(Debug, Deserialize)]
pub struct OllamaUsageResponse {
    limits: Option<OllamaLimits>,
}

#[derive(Debug, Deserialize)]
pub struct OllamaLimits {
    session: Option<OllamaUsageLimit>,
    weekly: Option<OllamaUsageLimit>,
}

#[derive(Debug, Deserialize)]
pub struct OllamaUsageLimit {
    usage: Option<f64>,
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
    for (label, limit) in [("5h", session), ("7d", weekly)] {
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_format_countdown_hours_and_minutes() {
        let now = Utc::now();
        let window = QuotaWindow {
            label: "5h".to_string(),
            used_percent: 7.0,
            resets_at: Some(now + Duration::hours(3) + Duration::minutes(22)),
            used_value: 7.0,
            limit_value: 100.0,
            is_currency: false,
            limited: false,
        };

        assert_eq!(window.format_countdown_at(now).unwrap(), "3h22m");
    }

    #[test]
    fn test_format_quota_windows_multiple() {
        let now = Utc::now();
        let windows = vec![
            QuotaWindow {
                label: "5h".to_string(),
                used_percent: 7.0,
                resets_at: Some(now + Duration::hours(3) + Duration::minutes(22)),
                used_value: 7.0,
                limit_value: 100.0,
                is_currency: false,
                limited: false,
            },
            QuotaWindow {
                label: "7d".to_string(),
                used_percent: 2.0,
                resets_at: Some(now + Duration::days(6) + Duration::hours(1)),
                used_value: 2.0,
                limit_value: 100.0,
                is_currency: false,
                limited: false,
            },
        ];

        let formatted = format_quota_windows(&windows).unwrap();
        assert!(formatted.contains("93% (3h22m)"));
        assert!(formatted.contains("98% (6d1h)"));
    }

    #[test]
    fn parse_ollama_usage_builds_session_and_weekly_windows() {
        let body = OllamaUsageResponse {
            limits: Some(OllamaLimits {
                session: Some(OllamaUsageLimit { usage: Some(0.313) }),
                weekly: Some(OllamaUsageLimit { usage: Some(0.445) }),
            }),
        };
        let windows = parse_ollama_usage(&body);
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].label, "5h");
        assert!((windows[0].used_percent - 31.3).abs() < 0.01);
        assert_eq!(windows[1].label, "7d");
        assert!((windows[1].used_percent - 44.5).abs() < 0.01);
        assert!(windows.iter().all(|w| w.resets_at.is_none()));
    }

    #[test]
    fn parse_ollama_usage_ignores_non_finite_usage() {
        let body = OllamaUsageResponse {
            limits: Some(OllamaLimits {
                session: Some(OllamaUsageLimit { usage: Some(f64::NAN) }),
                weekly: Some(OllamaUsageLimit { usage: Some(0.5) }),
            }),
        };
        let windows = parse_ollama_usage(&body);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].label, "7d");
    }

    #[test]
    fn parse_ollama_usage_requires_both_limits() {
        let body = OllamaUsageResponse {
            limits: Some(OllamaLimits {
                session: Some(OllamaUsageLimit { usage: Some(0.5) }),
                weekly: None,
            }),
        };
        assert!(parse_ollama_usage(&body).is_empty());
    }
}
