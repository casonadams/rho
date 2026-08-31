use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
            format!("{percent} {countdown}")
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

pub(crate) fn parse_reset_time(val: &Option<serde_json::Value>) -> Option<DateTime<Utc>> {
    match val {
        Some(serde_json::Value::String(s)) => DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&Utc)),
        Some(serde_json::Value::Number(n)) => {
            if let Some(ts) = n.as_i64() {
                DateTime::from_timestamp(ts, 0)
            } else if let Some(ts_f) = n.as_f64() {
                DateTime::from_timestamp(ts_f as i64, 0)
            } else {
                None
            }
        }
        _ => None,
    }
}
