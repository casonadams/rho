use crate::auth::claude::local::*;
use rho_harness_core::auth::StoredCredential;
use tempfile::tempdir;

#[test]
fn test_parse_credentials_iso_date() {
    let raw = r#"{
        "claudeAiOauth": {
            "accessToken": "sk-ant-oat01-test-token",
            "refreshToken": "sk-ant-ort01-refresh-token",
            "expiresAt": "2026-06-01T12:00:00.000Z",
            "scopes": ["user:inference"]
        }
    }"#;
    let (access, refresh, exp) = parse_claude_credentials_json(raw).unwrap();
    assert_eq!(access, "sk-ant-oat01-test-token");
    assert_eq!(refresh.as_deref(), Some("sk-ant-ort01-refresh-token"));
    assert!(exp.is_some());
    assert!(exp.unwrap() > 1700000000000);
}

#[test]
fn test_parse_credentials_numeric_ms() {
    let raw = r#"{
        "claudeAiOauth": {
            "accessToken": "sk-ant-oat01-numeric-token",
            "refreshToken": "sk-ant-ort01-numeric-refresh",
            "expiresAt": 1777684466674
        }
    }"#;
    let (access, refresh, exp) = parse_claude_credentials_json(raw).unwrap();
    assert_eq!(access, "sk-ant-oat01-numeric-token");
    assert_eq!(refresh.as_deref(), Some("sk-ant-ort01-numeric-refresh"));
    assert_eq!(exp, Some(1777684466674));
}

#[test]
fn test_parse_credentials_numeric_seconds() {
    let raw = r#"{
        "claudeAiOauth": {
            "accessToken": "sk-ant-oat01-sec-token",
            "expiresAt": 1800000000
        }
    }"#;
    let (access, refresh, exp) = parse_claude_credentials_json(raw).unwrap();
    assert_eq!(access, "sk-ant-oat01-sec-token");
    assert_eq!(refresh, None);
    assert_eq!(exp, Some(1800000000000));
}

#[test]
fn test_parse_credentials_flat_object() {
    let raw = r#"{
        "accessToken": "sk-ant-oat01-flat",
        "refreshToken": "sk-ant-ort01-flat-ref"
    }"#;
    let (access, refresh, exp) = parse_claude_credentials_json(raw).unwrap();
    assert_eq!(access, "sk-ant-oat01-flat");
    assert_eq!(refresh.as_deref(), Some("sk-ant-ort01-flat-ref"));
    assert_eq!(exp, None);
}

#[test]
fn test_parse_credentials_missing_access_token() {
    let raw = r#"{"claudeAiOauth": {"refreshToken": "sk-ant-ort01-only"}}"#;
    assert!(parse_claude_credentials_json(raw).is_none());

    let raw_empty = r#"{"claudeAiOauth": {"accessToken": "   "}}"#;
    assert!(parse_claude_credentials_json(raw_empty).is_none());
}

#[test]
fn test_parse_claude_json_metadata_valid() {
    let raw = r#"{
        "oauthAccount": {
            "accountUuid": "user-uuid-123",
            "organizationUuid": "org-uuid-456",
            "emailAddress": "dev@example.com"
        }
    }"#;
    let (id, email) = parse_claude_json_metadata(raw);
    assert_eq!(id.as_deref(), Some("org-uuid-456"));
    assert_eq!(email.as_deref(), Some("dev@example.com"));
}

#[test]
fn test_parse_claude_json_metadata_account_uuid_fallback() {
    let raw = r#"{
        "oauthAccount": {
            "accountUuid": "fallback-user-uuid",
            "emailAddress": "fallback@example.com"
        }
    }"#;
    let (id, email) = parse_claude_json_metadata(raw);
    assert_eq!(id.as_deref(), Some("fallback-user-uuid"));
    assert_eq!(email.as_deref(), Some("fallback@example.com"));
}

#[test]
fn test_parse_claude_json_metadata_missing() {
    let (id, email) = parse_claude_json_metadata(r#"{"other": 123}"#);
    assert_eq!(id, None);
    assert_eq!(email, None);
}

#[test]
fn test_detect_credentials_from_paths_success() {
    let dir = tempdir().unwrap();
    let creds_path = dir.path().join(".credentials.json");
    let config_path = dir.path().join(".claude.json");

    std::fs::write(
        &creds_path,
        r#"{"claudeAiOauth": {"accessToken": "test-access", "refreshToken": "test-refresh", "expiresAt": 1800000000000}}"#,
    ).unwrap();

    std::fs::write(
        &config_path,
        r#"{"oauthAccount": {"organizationUuid": "org-1", "emailAddress": "user@test.com"}}"#,
    )
    .unwrap();

    let cred = detect_credentials_from_paths(&creds_path, Some(&config_path)).unwrap();
    match cred {
        StoredCredential::OAuth {
            access_token,
            refresh_token,
            expires_at_ms,
            account_id,
            account_email,
        } => {
            assert_eq!(access_token, "test-access");
            assert_eq!(refresh_token.as_deref(), Some("test-refresh"));
            assert_eq!(expires_at_ms, Some(1800000000000));
            assert_eq!(account_id.as_deref(), Some("org-1"));
            assert_eq!(account_email.as_deref(), Some("user@test.com"));
        }
        _ => panic!("Expected OAuth credential"),
    }
}
