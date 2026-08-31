use super::*;
use chrono::{Duration, Utc};

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
            label: "7d".to_string(),
            used_percent: 2.0,
            resets_at: Some(now + Duration::days(6) + Duration::hours(1)),
            used_value: 2.0,
            limit_value: 100.0,
            is_currency: false,
            limited: false,
        },
        QuotaWindow {
            label: "5h".to_string(),
            used_percent: 7.0,
            resets_at: Some(now + Duration::hours(3) + Duration::minutes(22)),
            used_value: 7.0,
            limit_value: 100.0,
            is_currency: false,
            limited: false,
        },
    ];

    let formatted = format_quota_windows(&windows).unwrap();
    assert_eq!(formatted, "98% 6d1h 93% 3h22m");
}

#[test]
fn test_parse_antigravity_usage_groups_and_buckets() {
    let json = serde_json::json!({
        "groups": [
            {
                "displayName": "Gemini",
                "buckets": [
                    {
                        "displayName": "5 Hours",
                        "remainingFraction": 0.85,
                        "resetTime": "2026-08-31T03:00:00Z"
                    },
                    {
                        "displayName": "Weekly",
                        "remainingFraction": 0.95
                    }
                ]
            }
        ]
    });

    let windows = parse_antigravity_usage(&json, None);
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0].label, "Weekly");
    assert!((windows[0].remaining_percent() - 95.0).abs() < 0.01);
    assert_eq!(windows[1].label, "5 Hours");
    assert!((windows[1].remaining_percent() - 85.0).abs() < 0.01);
}

#[test]
fn test_parse_antigravity_usage_models_catalog() {
    let json = serde_json::json!({
        "models": {
            "gemini-3.7-flash": {
                "displayName": "Gemini 3.7 Flash",
                "quotaInfo": {
                    "displayName": "5 Hours",
                    "remainingFraction": 0.24,
                    "resetTime": "2026-08-31T03:00:00Z"
                }
            }
        }
    });

    let windows = parse_antigravity_usage(&json, Some("gemini-3.7-flash"));
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].label, "5h");
    assert!((windows[0].remaining_percent() - 24.0).abs() < 0.01);
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
    assert_eq!(windows[0].label, "7d");
    assert!((windows[0].used_percent - 44.5).abs() < 0.01);
    assert_eq!(windows[1].label, "5h");
    assert!((windows[1].used_percent - 31.3).abs() < 0.01);
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
