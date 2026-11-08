use super::*;
use crate::test_support::assert_quota_cases;
use chrono::TimeZone;

const PRIMARY_AND_SECONDARY: &str = r#"{"rate_limit":{"primary_window":{"remaining_percent":95.0,"reset_at":1788452580},"secondary_window":{"remaining_percent":89.0,"reset_at":"2026-09-07T09:00:00Z"}}}"#;
const ALTERNATE_KEYS: &str = r#"{"rate_limits":{"five_hour":{"used_percent":5.0,"reset_at":1788452580},"weekly":{"percent_left":89.0,"reset_at":"2026-09-07T09:00:00Z"}}}"#;
const PRIMARY_ONLY: &str = r#"{"rate_limit":{"primary_window":{"remaining_percent":95.0,"reset_at":1788452580}}}"#;
const EXPIRED_OR_MISSING_RESET: &str = r#"{"rate_limit":{"primary_window":{"remaining_percent":100.0,"reset_at":1788430000},"secondary_window":{"remaining_percent":90.0}}}"#;

const CASES: &[(&str, &str, Option<&str>, Option<&str>)] = &[
    (
        "formats_5h_then_weekly",
        PRIMARY_AND_SECONDARY,
        None,
        Some("5% 4h23m 11% 3d21h"),
    ),
    (
        "supports_used_percent_and_alternate_keys",
        ALTERNATE_KEYS,
        None,
        Some("5% 4h23m 11% 3d21h"),
    ),
    (
        "single_window_when_only_primary_present",
        PRIMARY_ONLY,
        None,
        Some("5% 4h23m"),
    ),
    (
        "expired_or_missing_reset_omits_countdown",
        EXPIRED_OR_MISSING_RESET,
        None,
        Some("0% 10%"),
    ),
    ("missing_rate_limit_returns_none", r#"{"other":{}}"#, None, None),
];

#[test]
fn parse_quota_cases() {
    let now = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap();
    assert_quota_cases(CASES, now, |value, _, now| parse_quota(value, now));
}

#[test]
fn test_friendly_error_formatting() {
    use super::client::friendly_error;
    assert_eq!(
        friendly_error(None, "ChatGPT request failed: timeout"),
        "ChatGPT request failed: timeout"
    );
    assert_eq!(
        friendly_error(None, "broken pipe"),
        "ChatGPT request failed: broken pipe"
    );
    assert!(friendly_error(Some(401), "").contains("Run 'rho login chatgpt'"));
}

#[tokio::test]
async fn fetch_quota_formats_usage_from_endpoint_and_rejects_failures() {
    let cases = [
        (
            "200 OK",
            r#"{"rate_limit":{"primary_window":{"remaining_percent":95.0}}}"#,
            Some("5%"),
        ),
        ("200 OK", r#"{"other":{}}"#, None),
        (
            "500 Internal Server Error",
            r#"{"rate_limit":{"primary_window":{"remaining_percent":95.0}}}"#,
            None,
        ),
    ];
    for (status, body, expected) in cases {
        let usage_url = crate::test_support::serve_http(vec![(status, body.to_string())]).await;
        assert_eq!(
            fetch_quota(&usage_url, "tok", Some("acct-1")).await.as_deref(),
            expected,
            "{status} {body}"
        );
    }
}
