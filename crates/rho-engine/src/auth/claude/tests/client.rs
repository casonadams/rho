use crate::auth::claude::client::*;

#[test]
fn test_claude_constants() {
    assert_eq!(CLIENT_ID, "9d1c250a-e61b-44d9-88ed-5944d1962f5e");
    assert_eq!(AUTHORIZE_URL, "https://claude.ai/oauth/authorize");
    assert_eq!(TOKEN_URL, "https://platform.claude.com/v1/oauth/token");
    assert_eq!(PROFILE_URL, "https://api.anthropic.com/api/oauth/profile");
    assert_eq!(REDIRECT_URI, "https://platform.claude.com/oauth/code/callback");
    assert!(SCOPES.contains("user:inference"));
    assert!(SCOPES.contains("user:profile"));
    assert!(SCOPES.contains("user:sessions:claude_code"));
    assert_eq!(USER_AGENT, "claude-cli/2.1.62");
    assert_eq!(TOKEN_TIMEOUT.as_secs(), 60);
}

#[test]
fn test_token_response_deserialization() {
    let json = r#"{
        "token_type": "Bearer",
        "access_token": "sk-ant-oat01-abc",
        "refresh_token": "sk-ant-ort01-xyz",
        "expires_in": 28800,
        "scope": "user:inference user:profile",
        "organization": {
            "uuid": "org-uuid-1",
            "name": "Org One"
        },
        "account": {
            "uuid": "acc-uuid-1",
            "email_address": "user@example.com"
        }
    }"#;

    let res: ClaudeTokenResponse = serde_json::from_str(json).unwrap();
    assert_eq!(res.access_token, "sk-ant-oat01-abc");
    assert_eq!(res.refresh_token.as_deref(), Some("sk-ant-ort01-xyz"));
    assert_eq!(res.expires_in, Some(28800));
    assert_eq!(
        res.organization.as_ref().and_then(|o| o.uuid.as_deref()),
        Some("org-uuid-1")
    );
    assert_eq!(
        res.organization.as_ref().and_then(|o| o.name.as_deref()),
        Some("Org One")
    );
    assert_eq!(
        res.account.as_ref().and_then(|a| a.email_address.as_deref()),
        Some("user@example.com")
    );
}

#[test]
fn test_token_response_minimal() {
    let json = r#"{"access_token": "sk-ant-oat01-minimal"}"#;
    let res: ClaudeTokenResponse = serde_json::from_str(json).unwrap();
    assert_eq!(res.access_token, "sk-ant-oat01-minimal");
    assert_eq!(res.refresh_token, None);
    assert_eq!(res.expires_in, None);
    assert!(res.organization.is_none());
    assert!(res.account.is_none());
}

#[test]
fn test_profile_response_deserialization() {
    let json = r#"{
        "account": {
            "uuid": "acc-123",
            "email_address": "test@example.com"
        },
        "organization": {
            "uuid": "org-456",
            "name": "Test Org"
        }
    }"#;

    let res: ClaudeProfileResponse = serde_json::from_str(json).unwrap();
    assert_eq!(res.account.as_ref().and_then(|a| a.uuid.as_deref()), Some("acc-123"));
    assert_eq!(
        res.account.as_ref().and_then(|a| a.email_address.as_deref()),
        Some("test@example.com")
    );
    assert_eq!(
        res.organization.as_ref().and_then(|o| o.uuid.as_deref()),
        Some("org-456")
    );
}

#[test]
fn test_clean_code_strip_fragment() {
    let input = "abc123code#fragment456";
    let clean = input.split_once('#').map(|(c, _)| c).unwrap_or(input).trim();
    assert_eq!(clean, "abc123code");
}
