use super::*;
use chrono::TimeZone;

#[test]
fn parse_quota_formats_5h_then_weekly() {
    let now = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap();
    let json = serde_json::json!({
        "rate_limit": {
            "primary_window": {
                "remaining_percent": 95.0,
                "reset_at": 1788452580
            },
            "secondary_window": {
                "remaining_percent": 89.0,
                "reset_at": "2026-09-07T09:00:00Z"
            }
        }
    });

    let display = parse_quota(&json, now);
    assert_eq!(display, Some("95% 4h23m 89% 3d21h".to_string()));
}

#[test]
fn parse_quota_supports_used_percent_and_alternate_keys() {
    let now = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap();
    let json = serde_json::json!({
        "rate_limits": {
            "five_hour": {
                "used_percent": 5.0,
                "reset_at": 1788452580
            },
            "weekly": {
                "percent_left": 89.0,
                "reset_at": "2026-09-07T09:00:00Z"
            }
        }
    });

    let display = parse_quota(&json, now);
    assert_eq!(display, Some("95% 4h23m 89% 3d21h".to_string()));
}

#[test]
fn parse_quota_single_window_when_only_primary_present() {
    let now = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap();
    let json = serde_json::json!({
        "rate_limit": {
            "primary_window": {
                "remaining_percent": 95.0,
                "reset_at": 1788452580
            }
        }
    });

    let display = parse_quota(&json, now);
    assert_eq!(display, Some("95% 4h23m".to_string()));
}

#[test]
fn parse_quota_expired_or_missing_reset_omits_countdown() {
    let now = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap();
    let json = serde_json::json!({
        "rate_limit": {
            "primary_window": {
                "remaining_percent": 100.0,
                "reset_at": 1788430000
            },
            "secondary_window": {
                "remaining_percent": 90.0
            }
        }
    });

    let display = parse_quota(&json, now);
    assert_eq!(display, Some("100% 90%".to_string()));
}

#[test]
fn parse_quota_missing_rate_limit_returns_none() {
    let now = Utc::now();
    let json = serde_json::json!({
        "other": {}
    });

    assert_eq!(parse_quota(&json, now), None);
}
