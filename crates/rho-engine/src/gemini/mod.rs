//! Gemini usage aggregation and billing window tracking for API key authentication.

use chrono::{DateTime, Datelike, NaiveDate, Utc};
use std::path::Path;

use crate::engine::SessionUsageTotals;

pub struct BillingWindows {
    pub start_of_day: DateTime<Utc>,
    pub start_of_month: DateTime<Utc>,
}

pub fn billing_windows(now: DateTime<Utc>) -> BillingWindows {
    let start_of_day = now.date_naive().and_hms_opt(0, 0, 0).unwrap_or_default().and_utc();
    let start_of_month = NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
        .unwrap_or_else(|| now.date_naive())
        .and_hms_opt(0, 0, 0)
        .unwrap_or_default()
        .and_utc();
    BillingWindows {
        start_of_day,
        start_of_month,
    }
}

pub fn calculate_gemini_cost(model: &str, input_tokens: u64, output_tokens: u64, cached_tokens: u64) -> f64 {
    let is_pro = model.to_ascii_lowercase().contains("pro");
    let (input_per_m, output_per_m, cached_per_m) = if is_pro {
        (1.25, 5.00, 0.3125)
    } else {
        (0.10, 0.40, 0.025)
    };
    (input_tokens as f64 * input_per_m + output_tokens as f64 * output_per_m + cached_tokens as f64 * cached_per_m)
        / 1_000_000.0
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct AggregatedCost {
    pub day_cost: f64,
    pub month_cost: f64,
}

impl AggregatedCost {
    pub fn format_status(&self) -> String {
        format!("${:.3} • ${:.3}", self.day_cost, self.month_cost)
    }
}

fn should_process_session_file(
    entry: &std::fs::DirEntry,
    skip_file_name: Option<&str>,
    start_of_month: DateTime<Utc>,
) -> bool {
    let path = entry.path();
    if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
        return false;
    }
    if let Some(skip_name) = skip_file_name
        && entry.file_name() == skip_name
    {
        return false;
    }
    if let Ok(meta) = entry.metadata()
        && let Ok(modified) = meta.modified()
    {
        let dt: DateTime<Utc> = modified.into();
        if dt < start_of_month {
            return false;
        }
    }
    true
}

fn parse_gemini_line_cost(line: &str, model: &str, windows: &BillingWindows) -> Option<(f64, f64)> {
    if !line.contains("\"audit_event\"") || !line.contains("\"run_summary\"") {
        return None;
    }
    let record = serde_json::from_str::<serde_json::Value>(line).ok()?;
    let event = record.get("event")?;
    let ts_str = event.get("timestamp")?.as_str()?;
    let ts_utc = DateTime::parse_from_rfc3339(ts_str).ok()?.with_timezone(&Utc);
    if ts_utc < windows.start_of_month {
        return None;
    }
    let usage = event.get("payload")?.get("usage")?;
    let input = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let output = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let cached = usage.get("cached_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let cost = calculate_gemini_cost(model, input, output, cached);
    let day = if ts_utc >= windows.start_of_day { cost } else { 0.0 };
    Some((day, cost))
}

fn accumulate_file_costs(path: &Path, model: &str, windows: &BillingWindows, day_cost: &mut f64, month_cost: &mut f64) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    for line in content.lines() {
        if let Some((day, month)) = parse_gemini_line_cost(line, model, windows) {
            *day_cost += day;
            *month_cost += month;
        }
    }
}

pub fn aggregate_gemini_usage(
    sessions_dir: &Path,
    current_session_id: Option<&str>,
    current_totals: &SessionUsageTotals,
    model: &str,
    now: DateTime<Utc>,
) -> AggregatedCost {
    let windows = billing_windows(now);
    let mut day_cost = 0.0;
    let mut month_cost = 0.0;

    if let Ok(entries) = std::fs::read_dir(sessions_dir) {
        let skip_file_name = current_session_id.map(|id| format!("{id}.jsonl"));
        for entry in entries.flatten() {
            if should_process_session_file(&entry, skip_file_name.as_deref(), windows.start_of_month) {
                accumulate_file_costs(&entry.path(), model, &windows, &mut day_cost, &mut month_cost);
            }
        }
    }

    let current_cost = calculate_gemini_cost(
        model,
        current_totals.total_input,
        current_totals.total_output,
        current_totals.total_cache_read,
    );
    day_cost += current_cost;
    month_cost += current_cost;

    AggregatedCost { day_cost, month_cost }
}

pub async fn fetch_quota(
    sessions_dir: &Path,
    current_session_id: Option<&str>,
    current_totals: &SessionUsageTotals,
    model: &str,
) -> Option<String> {
    let cost = aggregate_gemini_usage(sessions_dir, current_session_id, current_totals, model, Utc::now());
    Some(cost.format_status())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn billing_windows_computes_midnight_and_first_of_month() {
        let now = Utc.with_ymd_and_hms(2026, 9, 18, 14, 30, 0).unwrap();
        let windows = billing_windows(now);
        assert_eq!(
            windows.start_of_day,
            Utc.with_ymd_and_hms(2026, 9, 18, 0, 0, 0).unwrap()
        );
        assert_eq!(
            windows.start_of_month,
            Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn calculate_gemini_cost_differentiates_flash_and_pro() {
        let flash_cost = calculate_gemini_cost("gemini-2.5-flash", 1_000_000, 1_000_000, 1_000_000);
        assert!((flash_cost - 0.525).abs() < 1e-6);

        let pro_cost = calculate_gemini_cost("gemini-2.5-pro", 1_000_000, 1_000_000, 1_000_000);
        assert!((pro_cost - 6.5625).abs() < 1e-6);
    }

    #[test]
    fn aggregate_gemini_usage_empty_returns_zeroes() {
        let dir = std::env::temp_dir().join(format!("gemini_test_empty_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cost = aggregate_gemini_usage(
            &dir,
            None,
            &SessionUsageTotals::default(),
            "gemini-2.5-flash",
            Utc::now(),
        );
        assert_eq!(cost.format_status(), "$0.000 • $0.000");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn aggregate_gemini_usage_includes_current_session_and_skips_matching_id() {
        let dir = std::env::temp_dir().join(format!("gemini_test_agg_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 9, 18, 12, 0, 0).unwrap();

        // Write a past session from earlier today
        let past_record_today = serde_json::json!({
            "record_type": "audit_event",
            "sequence": 1,
            "session_id": "past-today",
            "event": {
                "id": "e1",
                "timestamp": "2026-09-18T08:00:00Z",
                "kind": "run_summary",
                "payload": {
                    "usage": {
                        "input_tokens": 100_000,
                        "output_tokens": 10_000,
                        "cached_input_tokens": 0
                    }
                }
            }
        });
        std::fs::write(dir.join("past-today.jsonl"), format!("{past_record_today}\n")).unwrap();

        // Write a past session from earlier this month (Sep 5)
        let past_record_month = serde_json::json!({
            "record_type": "audit_event",
            "sequence": 1,
            "session_id": "past-month",
            "event": {
                "id": "e2",
                "timestamp": "2026-09-05T08:00:00Z",
                "kind": "run_summary",
                "payload": {
                    "usage": {
                        "input_tokens": 200_000,
                        "output_tokens": 20_000,
                        "cached_input_tokens": 0
                    }
                }
            }
        });
        std::fs::write(dir.join("past-month.jsonl"), format!("{past_record_month}\n")).unwrap();

        // Current session file that should be skipped (in-memory totals used instead)
        let curr_record = serde_json::json!({
            "record_type": "audit_event",
            "sequence": 1,
            "session_id": "curr-session",
            "event": {
                "id": "e3",
                "timestamp": "2026-09-18T11:00:00Z",
                "kind": "run_summary",
                "payload": {
                    "usage": {
                        "input_tokens": 999_999,
                        "output_tokens": 999_999,
                        "cached_input_tokens": 0
                    }
                }
            }
        });
        std::fs::write(dir.join("curr-session.jsonl"), format!("{curr_record}\n")).unwrap();

        let current_totals = SessionUsageTotals {
            total_input: 50_000,
            total_output: 5_000,
            total_cache_read: 0,
            total_cache_write: 0,
            total_reasoning: 0,
        };

        // Flash pricing:
        // Today: past (100k in = 0.010, 10k out = 0.004 -> 0.014) + curr (50k in = 0.005, 5k out = 0.002 -> 0.007) = 0.021
        // Month: today (0.021) + earlier month (200k in = 0.020, 20k out = 0.008 -> 0.028) = 0.049
        let cost = aggregate_gemini_usage(&dir, Some("curr-session"), &current_totals, "gemini-2.5-flash", now);

        assert!(
            (cost.day_cost - 0.021).abs() < 1e-5,
            "expected ~0.021, got {}",
            cost.day_cost
        );
        assert!(
            (cost.month_cost - 0.049).abs() < 1e-5,
            "expected ~0.049, got {}",
            cost.month_cost
        );
        assert_eq!(cost.format_status(), "$0.021 • $0.049");

        let _ = std::fs::remove_dir_all(dir);
    }
}
