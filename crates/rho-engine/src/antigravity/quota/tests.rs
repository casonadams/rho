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

#[test]
fn parse_quota_summary_flat_buckets_isolates_gemini_from_unmetered_3p() {
    let now = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap();
    let json = serde_json::json!({
        "buckets": [
            {
                "bucketId": "gemini-5h",
                "displayName": "Five Hour Limit",
                "window": "5h",
                "remainingFraction": 0.70,
                "resetTime": "2026-09-03T15:00:00Z"
            },
            {
                "bucketId": "gemini-weekly",
                "displayName": "Weekly Limit",
                "window": "weekly",
                "remainingFraction": 0.85,
                "resetTime": "2026-09-09T12:00:00Z"
            },
            {
                "bucketId": "3p-5h",
                "displayName": "Third-Party 5h Limit",
                "window": "5h",
                "remainingFraction": 1.0,
                "resetTime": "2026-09-03T17:00:00Z"
            }
        ]
    });

    let display = parse_quota(&json, "gemini-2.5-pro", now);
    assert_eq!(display, Some("70% 3h0m 85% 6d0h".to_string()));
}

#[test]
fn parse_quota_summary_flat_buckets_isolates_claude_from_gemini() {
    let now = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap();
    let json = serde_json::json!({
        "buckets": [
            {
                "bucketId": "gemini-5h",
                "displayName": "Gemini 5h Limit",
                "window": "5h",
                "remainingFraction": 1.0,
                "resetTime": "2026-09-03T17:00:00Z"
            },
            {
                "bucketId": "3p-5h",
                "displayName": "Claude 5h Limit",
                "window": "5h",
                "remainingFraction": 0.60,
                "resetTime": "2026-09-03T14:30:00Z"
            },
            {
                "bucketId": "3p-weekly",
                "displayName": "Claude Weekly",
                "window": "weekly",
                "remainingFraction": 0.80,
                "resetTime": "2026-09-08T12:00:00Z"
            }
        ]
    });

    let display = parse_quota(&json, "claude-sonnet-4-6", now);
    assert_eq!(display, Some("60% 2h30m 80% 5d0h".to_string()));
}

#[test]
fn parse_quota_summary_weekly_description_mentioning_5h_is_not_misclassified() {
    let now = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap();
    let json = serde_json::json!({
        "buckets": [
            {
                "bucketId": "gemini-5h",
                "displayName": "Five Hour Limit Remaining",
                "remainingFraction": 0.0,
                "resetTime": "2026-09-03T12:20:00Z",
                "description": "You have hit your 5-hour limit, it will refresh in 20 minutes."
            },
            {
                "bucketId": "gemini-weekly",
                "displayName": "Weekly Limit Remaining",
                "remainingFraction": 0.50,
                "resetTime": "2026-09-07T12:00:00Z",
                "description": "You have hit your 5-hour limit, so the weekly limit does not currently apply."
            }
        ]
    });

    let display = parse_quota(&json, "gemini-2.5-pro", now);
    assert_eq!(display, Some("0% 20m 50% 4d0h".to_string()));
}

#[test]
fn parse_quota_summary_matches_group_by_group_id() {
    let now = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap();
    let json = serde_json::json!({
        "groups": [
            {
                "groupId": "gemini-models",
                "buckets": [
                    { "window": "5h", "remainingFraction": 0.40, "resetTime": "2026-09-03T13:00:00Z" }
                ]
            }
        ]
    });

    let display = parse_quota(&json, "gemini-2.5-flash", now);
    assert_eq!(display, Some("40% 1h0m".to_string()));
}

#[test]
fn parse_quota_summary_matches_gpt_and_custom_target() {
    let now = Utc::now();
    let json = serde_json::json!({
        "groups": [
            {
                "groupId": "gemini-models",
                "buckets": [
                    { "window": "5h", "remainingFraction": 1.0, "resetTime": (now + Duration::hours(5)).to_rfc3339() }
                ]
            },
            {
                "groupId": "gpt-other-models",
                "buckets": [
                    { "window": "5h", "remainingFraction": 0.65, "resetTime": (now + Duration::hours(5)).to_rfc3339() }
                ]
            },
            {
                "groupId": "deepseek-coder",
                "buckets": [
                    { "window": "5h", "remainingFraction": 0.40, "resetTime": (now + Duration::hours(5)).to_rfc3339() }
                ]
            }
        ]
    });

    let gpt_display = parse_quota_summary(&json, "gpt-oss-1", now);
    assert!(gpt_display.is_some());
    assert!(gpt_display.unwrap().contains("65%"));

    let custom_display = parse_quota_summary(&json, "deepseek-coder", now);
    assert!(custom_display.is_some());
    assert!(custom_display.unwrap().contains("40%"));
}

#[test]
fn unmetered_summary_detection() {
    assert!(is_unmetered_summary("100%"));
    assert!(is_unmetered_summary("100% 100%"));
    assert!(!is_unmetered_summary(""));
    assert!(!is_unmetered_summary("100% 4h30m"));
    assert!(!is_unmetered_summary("85% 3h20m 92% 5d12h"));
    assert!(!is_unmetered_summary("0% 20m"));
}

#[test]
fn parse_quota_unmetered_summary_falls_back_to_active_model_quota() {
    let now = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap();
    let json = serde_json::json!({
        "buckets": [
            {
                "bucketId": "gemini-5h",
                "remainingFraction": 1.0
            },
            {
                "bucketId": "gemini-weekly",
                "remainingFraction": 1.0
            }
        ],
        "models": {
            "gemini-2.5-pro": {
                "quotaInfo": {
                    "remainingFraction": 0.80,
                    "resetTime": "2026-09-03T14:15:00Z"
                }
            }
        }
    });

    let display = parse_quota(&json, "gemini-2.5-pro", now);
    assert_eq!(display, Some("80% 2h15m".to_string()));
}

#[tokio::test]
async fn fetch_quota_uses_summary_when_metered() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let reset_time = (Utc::now() + Duration::hours(3)).to_rfc3339();
    let mock_summary = serde_json::json!({
        "buckets": [
            {
                "bucketId": "gemini-5h",
                "remainingFraction": 0.85,
                "resetTime": reset_time
            }
        ]
    })
    .to_string();
    let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{mock_summary}");
    let addr = {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf).await;
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });
        addr
    };

    let endpoints = vec![format!("http://{addr}")];
    let quota = fetch_quota_from_endpoints(&endpoints, "token", "test-proj", "gemini-2.5-pro").await;
    assert!(quota.is_some());
    assert!(quota.unwrap().contains("85%"));
}

#[tokio::test]
async fn fetch_quota_falls_back_to_models_when_summary_empty() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let reset_time = (Utc::now() + Duration::hours(4)).to_rfc3339();
    let mock_models = serde_json::json!({
        "models": {
            "gemini-2.5-pro": {
                "quotaInfo": {
                    "remainingFraction": 0.70,
                    "resetTime": reset_time
                }
            }
        }
    })
    .to_string();

    let resp_404 = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    let resp_models =
        format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{mock_models}");

    let addr = {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            // First request to retrieveUserQuotaSummary fails with 404
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf).await;
                let _ = stream.write_all(resp_404.as_bytes()).await;
            }
            // Second request to fetchAvailableModels succeeds
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf).await;
                let _ = stream.write_all(resp_models.as_bytes()).await;
            }
        });
        addr
    };

    let endpoints = vec![format!("http://{addr}")];
    let quota = fetch_quota_from_endpoints(&endpoints, "token", "", "gemini-2.5-pro").await;
    assert!(quota.is_some());
    assert!(quota.unwrap().contains("70%"));
}
