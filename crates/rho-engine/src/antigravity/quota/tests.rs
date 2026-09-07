use super::*;
use chrono::TimeZone;

#[test]
fn format_duration_matches_all_breakpoints() {
    let cases = [
        (Duration::seconds(45), "45s"),
        (Duration::minutes(15), "15m"),
        (Duration::hours(3) + Duration::minutes(22), "3h22m"),
        (Duration::days(1) + Duration::hours(5), "1d5h"),
        (Duration::days(6) + Duration::hours(12), "6d12h"),
    ];
    for (d, expected) in cases {
        assert_eq!(format_duration(d), expected);
    }
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
    assert_eq!(display, Some("85% 3h22m".to_string()));
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
    assert_eq!(display, Some("74% 2h30m".to_string()));
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
    assert_eq!(display, Some("40% 1h0m".to_string()));
}

#[test]
fn parse_quota_summary_formats_5h_then_weekly() {
    let now = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap();
    let json = serde_json::json!({
        "groups": [
            {
                "displayName": "Gemini",
                "buckets": [
                    {
                        "displayName": "5 Hours",
                        "remainingFraction": 0.95,
                        "resetTime": "2026-09-03T16:23:00Z"
                    },
                    {
                        "displayName": "Weekly",
                        "remainingFraction": 0.89,
                        "resetTime": "2026-09-07T09:00:00Z"
                    }
                ]
            }
        ]
    });

    let display = parse_quota(&json, "gemini-2.5-pro", now);
    assert_eq!(display, Some("95% 4h23m 89% 3d21h".to_string()));
}

fn claude_groups_fixture() -> Value {
    serde_json::json!({
        "groups": [
            {
                "displayName": "Gemini Models",
                "buckets": [{ "window": "5h", "remainingFraction": 1.0, "resetTime": "2026-09-03T17:00:00Z" }]
            },
            {
                "displayName": "Claude & 3P Models",
                "buckets": [
                    { "window": "5h", "remainingFraction": 0.70, "resetTime": "2026-09-03T15:00:00Z" },
                    { "window": "weekly", "remainingFraction": 0.85, "resetTime": "2026-09-09T12:00:00Z" }
                ]
            }
        ]
    })
}

#[test]
fn parse_quota_summary_matches_claude_group() {
    let now = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap();
    let display = parse_quota(&claude_groups_fixture(), "claude-sonnet-4-6", now);
    assert_eq!(display, Some("70% 3h0m 85% 6d0h".to_string()));
}

#[test]
fn parse_quota_summary_flat_buckets_fallback() {
    let now = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap();
    let json = serde_json::json!({
        "buckets": [
            {
                "bucketId": "session",
                "displayName": "5h",
                "remainingFraction": 0.95,
                "resetTime": "2026-09-03T16:23:00Z"
            },
            {
                "bucketId": "weekly",
                "displayName": "Weekly",
                "remainingFraction": 0.89,
                "resetTime": "2026-09-07T09:00:00Z"
            }
        ]
    });

    let display = parse_quota(&json, "gemini-3.7-flash", now);
    assert_eq!(display, Some("95% 4h23m 89% 3d21h".to_string()));
}
