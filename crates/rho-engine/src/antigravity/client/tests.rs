use super::discovery::{extract_project_id, is_selectable_runtime_model};
use super::http::{antigravity_headers, friendly_error};

#[test]
fn is_selectable_runtime_model_filters_correctly() {
    assert!(is_selectable_runtime_model("gemini-2.5-pro"));
    assert!(is_selectable_runtime_model("gemini-3.7-flash"));
    assert!(is_selectable_runtime_model("claude-sonnet-4-6"));
    assert!(is_selectable_runtime_model("gpt-oss-1"));

    // Excluded patterns
    assert!(!is_selectable_runtime_model("gemini-image-gen"));
    assert!(!is_selectable_runtime_model("gemini-2.5 chat"));
    assert!(!is_selectable_runtime_model("MODEL_GEMINI_1"));
    assert!(!is_selectable_runtime_model("text-embedding-004"));
    assert!(!is_selectable_runtime_model("chat-bison-001"));
}

#[test]
fn extract_project_id_from_direct_fields() {
    let json1 = serde_json::json!({ "antigravityProjectId": "proj-anti-1" });
    assert_eq!(extract_project_id(&json1), Some("proj-anti-1".to_string()));

    let json2 = serde_json::json!({ "projectId": "proj-2" });
    assert_eq!(extract_project_id(&json2), Some("proj-2".to_string()));

    let json3 = serde_json::json!({ "backendProjectId": "proj-3" });
    assert_eq!(extract_project_id(&json3), Some("proj-3".to_string()));

    let json4 = serde_json::json!({ "cloudaicompanionProject": "proj-4" });
    assert_eq!(extract_project_id(&json4), Some("proj-4".to_string()));
}

#[test]
fn extract_project_id_from_nested_arrays() {
    let json_str_array = serde_json::json!({
        "projects": ["first-proj", "second-proj"]
    });
    assert_eq!(extract_project_id(&json_str_array), Some("first-proj".to_string()));

    let json_nested_obj = serde_json::json!({
        "cloudaicompanionProjects": [
            { "projectId": "nested-proj" }
        ]
    });
    assert_eq!(extract_project_id(&json_nested_obj), Some("nested-proj".to_string()));

    let json_empty = serde_json::json!({});
    assert_eq!(extract_project_id(&json_empty), None);
}

#[test]
fn friendly_error_formats_known_error_cases() {
    let quota_body = r#"{"error":{"message":"Individual quota reached. Resets in 2h45m."}}"#;
    let quota_err = friendly_error(Some(429), quota_body);
    assert!(quota_err.contains("Resets in 2h45m"));

    let rate_limit_body = r#"{"error":{"message":"Resource has been exhausted (e.g. check quota)."}}"#;
    let rate_err = friendly_error(Some(429), rate_limit_body);
    assert!(rate_err.contains("rate limit reached"));

    let auth_err = friendly_error(Some(401), "Unauthorized");
    assert!(auth_err.contains("rho login antigravity"));

    let forbidden = friendly_error(Some(403), r#"{"error":{"message":"Permission denied"}}"#);
    assert!(forbidden.contains("access denied"));
    assert!(forbidden.contains("Permission denied"));

    let not_found = friendly_error(Some(404), r#"{"error":{"message":"Model not found"}}"#);
    assert!(not_found.contains("Model not available"));

    let capacity = friendly_error(Some(503), r#"{"error":{"message":"No capacity available"}}"#);
    assert!(capacity.contains("no capacity right now"));

    let generic_500 = friendly_error(Some(500), r#"{"error":{"message":"Internal server error"}}"#);
    assert!(generic_500.contains("API error (500)"));

    let none_status = friendly_error(None, "Connection closed");
    assert!(none_status.contains("Antigravity request failed: Connection closed"));
}

#[test]
fn antigravity_headers_sets_expected_keys() {
    let headers = antigravity_headers("test-secret-token");
    assert_eq!(
        headers.get("authorization").and_then(|v| v.to_str().ok()),
        Some("Bearer test-secret-token")
    );
    assert_eq!(
        headers.get("content-type").and_then(|v| v.to_str().ok()),
        Some("application/json")
    );
    assert!(headers.get("user-agent").is_some());
    assert!(headers.get("x-goog-api-client").is_some());
    assert!(headers.get("client-metadata").is_some());
}
