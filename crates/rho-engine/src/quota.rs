//! Rolling-window quota display shared by the provider usage endpoints
//! (Claude, ChatGPT, Antigravity).

use chrono::{DateTime, Duration, Utc};
use serde_json::Value;

pub(crate) enum Percent {
    Remaining(&'static str),
    Used(&'static str),
}

pub(crate) struct WindowSpec {
    pub(crate) percent: &'static [Percent],
    pub(crate) reset: &'static [&'static str],
}

pub(crate) fn format_window(window: &Value, spec: &WindowSpec, now: DateTime<Utc>) -> Option<String> {
    let fraction = remaining_fraction(window, spec.percent)?;
    let reset_time = parse_reset_time(spec.reset.iter().find_map(|key| window.get(key)));
    Some(format_quota_window(fraction, reset_time, now))
}

fn remaining_fraction(window: &Value, keys: &[Percent]) -> Option<f64> {
    keys.iter().find_map(|key| match key {
        Percent::Remaining(name) => window.get(name)?.as_f64().map(|r| r / 100.0),
        Percent::Used(name) => window.get(name)?.as_f64().map(|u| (100.0 - u).max(0.0) / 100.0),
    })
}

fn parse_reset_time(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let value = value?;
    if let Some(s) = value.as_str() {
        DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&Utc))
    } else if let Some(ts) = value.as_i64() {
        DateTime::from_timestamp(ts, 0)
    } else {
        DateTime::from_timestamp(value.as_f64()? as i64, 0)
    }
}

pub(crate) fn format_quota_window(
    remaining_fraction: f64,
    reset_time: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> String {
    let pct = 100 - (remaining_fraction * 100.0).round().clamp(0.0, 100.0) as u64;
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    const WINDOW: WindowSpec = WindowSpec {
        percent: &[Percent::Used("utilization"), Percent::Remaining("remaining_percent")],
        reset: &["resets_at", "reset_at"],
    };

    #[test]
    fn format_window_prefers_earlier_percent_keys_and_falls_back_to_reset_alias() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let window = json!({ "utilization": 25.0, "remaining_percent": 10.0, "reset_at": now.timestamp() + 3600 });
        assert_eq!(format_window(&window, &WINDOW, now).as_deref(), Some("25% 1h0m"));
    }

    #[test]
    fn format_window_clamps_used_percent_at_zero_remaining() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let window = json!({ "utilization": 120.0 });
        assert_eq!(format_window(&window, &WINDOW, now).as_deref(), Some("100%"));
    }

    #[test]
    fn parse_reset_time_accepts_rfc3339_epoch_and_float_seconds() {
        let expected = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(parse_reset_time(Some(&json!("2026-01-01T00:00:00Z"))), Some(expected));
        assert_eq!(parse_reset_time(Some(&json!(expected.timestamp()))), Some(expected));
        assert_eq!(
            parse_reset_time(Some(&json!(expected.timestamp() as f64))),
            Some(expected)
        );
        assert_eq!(parse_reset_time(Some(&json!(true))), None);
    }
}
