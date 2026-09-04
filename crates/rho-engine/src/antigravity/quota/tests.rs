use super::*;
use chrono::TimeZone;

#[test]
fn format_duration_matches_all_breakpoints() {
    assert_eq!(format_duration(Duration::seconds(45)), "45s");
    assert_eq!(format_duration(Duration::minutes(15)), "15m");
    assert_eq!(format_duration(Duration::hours(3) + Duration::minutes(22)), "3h22m");
    assert_eq!(format_duration(Duration::days(1) + Duration::hours(5)), "1d5h");
    assert_eq!(format_duration(Duration::days(6) + Duration::hours(12)), "6d12h");
}

#[test]
fn parse_quota_exact_match_with_reset_time() {
    let now = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap();
    let json = serde_json::json!({
        "models": {
            "gemini-2.5-pro": {
                "quotaInfo": {
                    "remainingFraction": 0.85,
                    "resetTime": "2026-09-03T15:22:00Z"
                }
            }
        }
    });

    let display = parse_quota(&json, "gemini-2.5-pro", now);
    assert_eq!(display, Some("85% (3h22m)".to_string()));
}

#[test]
fn parse_quota_prefix_match_picks_lowest_fraction() {
    let now = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap();
    let json = serde_json::json!({
        "models": {
            "claude-sonnet-4-6-high": {
                "quotaInfo": {
                    "remainingFraction": 0.90,
                    "resetTime": "2026-09-03T16:00:00Z"
                }
            },
            "claude-sonnet-4-6-low": {
                "quotaInfo": {
                    "remainingFraction": 0.74,
                    "resetTime": "2026-09-03T14:30:00Z"
                }
            }
        }
    });

    let display = parse_quota(&json, "claude-sonnet-4-6", now);
    assert_eq!(display, Some("74% (2h30m)".to_string()));
}

#[test]
fn parse_quota_expired_reset_time_omits_countdown() {
    let now = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap();
    let json = serde_json::json!({
        "models": {
            "gemini-2.5-flash": {
                "quotaInfo": {
                    "remainingFraction": 1.0,
                    "resetTime": "2026-09-03T11:00:00Z"
                }
            }
        }
    });

    let display = parse_quota(&json, "gemini-2.5-flash", now);
    assert_eq!(display, Some("100%".to_string()));
}

#[test]
fn parse_quota_missing_quota_info_returns_none() {
    let now = Utc::now();
    let json = serde_json::json!({
        "models": {
            "gemini-2.5-pro": {}
        }
    });

    assert_eq!(parse_quota(&json, "gemini-2.5-pro", now), None);
}

#[test]
fn parse_quota_fallback_to_lowest_when_no_name_matches() {
    let now = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap();
    let json = serde_json::json!({
        "models": {
            "model-a": {
                "quotaInfo": {
                    "remainingFraction": 0.95,
                    "resetTime": "2026-09-03T18:00:00Z"
                }
            },
            "model-b": {
                "quotaInfo": {
                    "remainingFraction": 0.40,
                    "resetTime": "2026-09-03T13:00:00Z"
                }
            }
        }
    });

    let display = parse_quota(&json, "completely-different-model", now);
    assert_eq!(display, Some("40% (1h0m)".to_string()));
}
