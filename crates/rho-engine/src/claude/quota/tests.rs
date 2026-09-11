use super::*;
use chrono::TimeZone;
use serde_json::json;

#[test]
fn parse_quota_formats_5h_then_weekly() {
    let now = Utc.with_ymd_and_hms(2026, 9, 11, 12, 0, 0).unwrap();
    let json = json!({
        "five_hour": {
            "utilization": 15.0,
            "resets_at": "2026-09-11T16:30:00Z"
        },
        "seven_day": {
            "utilization": 40.0,
            "resets_at": "2026-09-15T12:00:00Z"
        }
    });

    let display = parse_quota(&json, None, now);
    assert_eq!(display, Some("85% 4h30m 60% 4d0h".to_string()));
}

#[test]
fn parse_quota_model_specific_overrides() {
    let now = Utc.with_ymd_and_hms(2026, 9, 11, 12, 0, 0).unwrap();
    let json = json!({
        "five_hour": {
            "utilization": 0.0,
            "resets_at": "2026-09-11T15:00:00Z"
        },
        "seven_day": {
            "utilization": 50.0,
            "resets_at": "2026-09-16T12:00:00Z"
        },
        "seven_day_opus": {
            "utilization": 20.0,
            "resets_at": "2026-09-14T12:00:00Z"
        }
    });

    let display_opus = parse_quota(&json, Some("claude-opus-4-6"), now);
    assert_eq!(display_opus, Some("100% 3h0m 80% 3d0h".to_string()));

    let display_sonnet = parse_quota(&json, Some("claude-sonnet-4-6"), now);
    assert_eq!(display_sonnet, Some("100% 3h0m 50% 5d0h".to_string()));
}

#[test]
fn parse_quota_single_window_when_only_five_hour_present() {
    let now = Utc.with_ymd_and_hms(2026, 9, 11, 12, 0, 0).unwrap();
    let json = json!({
        "five_hour": {
            "utilization": 10.0,
            "resets_at": "2026-09-11T14:15:00Z"
        },
        "seven_day": null
    });

    let display = parse_quota(&json, None, now);
    assert_eq!(display, Some("90% 2h15m".to_string()));
}

#[test]
fn parse_quota_missing_fields_returns_none() {
    let now = Utc::now();
    let json = json!({});
    assert_eq!(parse_quota(&json, None, now), None);
}
