use crate::auth::claude::client::*;

#[test]
fn test_claude_constants() {
    let urls = (
        CLIENT_ID,
        AUTHORIZE_URL,
        TOKEN_URL,
        PROFILE_URL,
        REDIRECT_URI,
        USER_AGENT,
        TOKEN_TIMEOUT.as_secs(),
    );
    assert_eq!(
        urls,
        (
            "9d1c250a-e61b-44d9-88ed-5944d1962f5e",
            "https://claude.com/cai/oauth/authorize",
            "https://platform.claude.com/v1/oauth/token",
            "https://api.anthropic.com/api/oauth/profile",
            "https://platform.claude.com/oauth/code/callback",
            "claude-cli/2.1.226 (external, cli)",
            60,
        )
    );
    for s in ["user:inference", "user:profile", "user:sessions:claude_code"] {
        assert!(SCOPES.contains(s));
    }
}

#[test]
fn test_token_response_deserialization() {
    let json = r#"{"token_type":"Bearer","access_token":"sk-ant-oat01-abc","refresh_token":"sk-ant-ort01-xyz","expires_in":28800,"organization":{"uuid":"org-uuid-1","name":"Org One"},"account":{"uuid":"acc-uuid-1","email_address":"user@example.com"}}"#;
    let res: ClaudeTokenResponse = serde_json::from_str(json).unwrap();
    let org = res.organization.as_ref();
    let acc = res.account.as_ref();
    let actual = (
        res.access_token.as_str(),
        res.refresh_token.as_deref(),
        res.expires_in,
        org.and_then(|o| o.uuid.as_deref()),
        org.and_then(|o| o.name.as_deref()),
        acc.and_then(|a| a.email_address.as_deref()),
    );
    assert_eq!(
        actual,
        (
            "sk-ant-oat01-abc",
            Some("sk-ant-ort01-xyz"),
            Some(28800),
            Some("org-uuid-1"),
            Some("Org One"),
            Some("user@example.com")
        )
    );
}

#[test]
fn test_token_response_minimal() {
    let json = r#"{"access_token": "sk-ant-oat01-minimal"}"#;
    let res: ClaudeTokenResponse = serde_json::from_str(json).unwrap();
    assert_eq!(
        (res.access_token.as_str(), res.refresh_token, res.expires_in),
        ("sk-ant-oat01-minimal", None, None)
    );
    assert!(res.organization.is_none() && res.account.is_none());
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
