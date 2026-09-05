use super::parse_quota;
use serde_json::json;

#[test]
fn parse_quota_formats_monthly_usage_fraction() {
    let payload = json!({
        "activity": {
            "cost": "0.00000",
            "period": { "type": "last_4_weeks", "starting_at": "2026-08-10T00:00:00Z" },
            "models": []
        },
        "limits": {
            "monthly": {
                "usage": 0.197,
                "models": [
                    { "name": "glm-5.3-flash", "request_count": 2244 },
                    { "name": "gemma4:31b", "request_count": 23 }
                ]
            }
        }
    });
    assert_eq!(parse_quota(&payload), Some("20% used".to_string()));
}

#[test]
fn parse_quota_clamps_and_requires_limits_monthly_usage() {
    assert_eq!(
        parse_quota(&json!({ "limits": { "monthly": { "usage": 1.4 } } })),
        Some("100% used".to_string())
    );
    assert_eq!(parse_quota(&json!({ "limits": { "monthly": {} } })), None);
    assert_eq!(parse_quota(&json!({ "limits": {} })), None);
    assert_eq!(parse_quota(&json!({})), None);
}
