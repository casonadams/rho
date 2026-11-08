use super::*;
use crate::test_support::assert_quota_cases;
use chrono::TimeZone;

const FIVE_HOUR_AND_WEEKLY: &str = r#"{"five_hour":{"utilization":15.0,"resets_at":"2026-09-11T16:30:00Z"},"seven_day":{"utilization":40.0,"resets_at":"2026-09-15T12:00:00Z"}}"#;
const MODEL_WINDOWS: &str = r#"{"five_hour":{"utilization":0.0,"resets_at":"2026-09-11T15:00:00Z"},"seven_day":{"utilization":50.0,"resets_at":"2026-09-16T12:00:00Z"},"seven_day_opus":{"utilization":20.0,"resets_at":"2026-09-14T12:00:00Z"}}"#;
const FIVE_HOUR_ONLY: &str =
    r#"{"five_hour":{"utilization":10.0,"resets_at":"2026-09-11T14:15:00Z"},"seven_day":null}"#;

const CASES: &[(&str, &str, Option<&str>, Option<&str>)] = &[
    (
        "formats_5h_then_weekly",
        FIVE_HOUR_AND_WEEKLY,
        None,
        Some("15% 4h30m 40% 4d0h"),
    ),
    (
        "opus_model_specific_override",
        MODEL_WINDOWS,
        Some("claude-opus-4-6"),
        Some("0% 3h0m 20% 3d0h"),
    ),
    (
        "sonnet_model_specific_override",
        MODEL_WINDOWS,
        Some("claude-sonnet-4-6"),
        Some("0% 3h0m 50% 5d0h"),
    ),
    (
        "single_window_when_only_five_hour_present",
        FIVE_HOUR_ONLY,
        None,
        Some("10% 2h15m"),
    ),
    ("missing_fields_returns_none", "{}", None, None),
];

#[test]
fn parse_quota_cases() {
    let now = Utc.with_ymd_and_hms(2026, 9, 11, 12, 0, 0).unwrap();
    assert_quota_cases(CASES, now, parse_quota);
}

#[tokio::test]
async fn fetch_quota_reads_usage_endpoint_and_rejects_failures() {
    let cases = [
        ("200 OK", FIVE_HOUR_ONLY, Some("10%")),
        ("200 OK", "{}", None),
        ("503 Service Unavailable", FIVE_HOUR_ONLY, None),
    ];
    for (status, body, expected_prefix) in cases {
        let usage_url = crate::test_support::serve_http(vec![(status, body.to_string())]).await;
        let display = fetch_quota(&usage_url, "tok", None).await;
        match expected_prefix {
            Some(prefix) => assert!(display.is_some_and(|d| d.starts_with(prefix)), "{status} {body}"),
            None => assert_eq!(display, None, "{status} {body}"),
        }
    }
}
