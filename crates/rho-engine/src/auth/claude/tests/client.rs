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
            "claude-cli/2.1.280",
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

#[tokio::test]
async fn exchange_code_with_redirect_maps_token_payload() {
    let token_url = crate::test_support::serve_http(vec![(
        "200 OK",
        r#"{"access_token":"sk-ant-oat-1","refresh_token":"sk-ant-ort-1","expires_in":28800,"scope":"user:inference"}"#
            .to_string(),
    )])
    .await;
    let token = exchange_code_with_redirect(&token_url, "abc123#state-9", "verifier", REDIRECT_URI, None)
        .await
        .unwrap();
    assert_eq!(token.access_token, "sk-ant-oat-1");
    assert_eq!(token.refresh_token.as_deref(), Some("sk-ant-ort-1"));
    assert_eq!(token.expires_in, Some(28800));
}

#[tokio::test]
async fn exchange_code_with_redirect_rejects_failed_or_malformed_responses() {
    let cases = [
        (
            "400 Bad Request",
            r#"{"error":"invalid_grant"}"#,
            "Claude token exchange failed (400",
        ),
        ("200 OK", "not json", "Failed to parse Claude token response"),
    ];
    for (status, body, expected) in cases {
        let token_url = crate::test_support::serve_http(vec![(status, body.to_string())]).await;
        let err = exchange_code_with_redirect(&token_url, "abc123", "verifier", REDIRECT_URI, Some("s"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains(expected), "{status}: {err}");
    }
}

#[tokio::test]
async fn refresh_token_returns_access_token_without_rotating_refresh() {
    let token_url = crate::test_support::serve_http(vec![(
        "200 OK",
        r#"{"access_token":"sk-ant-oat-2","expires_in":3600}"#.to_string(),
    )])
    .await;
    let token = refresh_token(&token_url, "sk-ant-ort-1").await.unwrap();
    assert_eq!(token.access_token, "sk-ant-oat-2");
    assert_eq!(token.refresh_token, None);
}

#[tokio::test]
async fn fetch_profile_returns_account_and_organization_on_success_only() {
    let profile_body = r#"{"account":{"uuid":"acct-1","email_address":"dev@example.com"},"organization":{"uuid":"org-1","name":"Acme"}}"#;
    let cases = [
        ("200 OK", profile_body, true),
        ("500 Internal Server Error", profile_body, false),
        ("200 OK", "not json", false),
    ];
    for (status, body, expect_profile) in cases {
        let profile_url = crate::test_support::serve_http(vec![(status, body.to_string())]).await;
        let profile = fetch_profile(&profile_url, "sk-ant-oat-1").await;
        assert_eq!(profile.is_some(), expect_profile, "{status} {body}");
        if let Some(profile) = profile {
            let account = profile.account.unwrap();
            assert_eq!(account.email_address.as_deref(), Some("dev@example.com"));
            assert_eq!(profile.organization.unwrap().name.as_deref(), Some("Acme"));
        }
    }
}
