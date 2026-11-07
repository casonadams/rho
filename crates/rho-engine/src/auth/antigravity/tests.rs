use super::*;

#[test]
fn stable_project_id_is_deterministic() {
    let a = stable_project_id("user@example.com");
    let b = stable_project_id("user@example.com");
    let c = stable_project_id("other@example.com");
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn stable_project_id_is_uuid_shaped() {
    let a = stable_project_id("user@example.com");
    let parts: Vec<_> = a.split('-').collect();
    assert_eq!((parts.len(), parts[0].len(), parts[1].len()), (5, 8, 4));
}

#[test]
fn google_auth_url_carries_pkce_state_and_offline_access() {
    let url = url::Url::parse(&build_google_auth_url("challenge-xyz", "state-abc")).unwrap();
    let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(url.origin().ascii_serialization(), "https://accounts.google.com");
    assert_eq!(query["code_challenge"], "challenge-xyz");
    assert_eq!(query["state"], "state-abc");
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(query["redirect_uri"], "http://localhost:51121/oauth-callback");
    assert_eq!(query["access_type"], "offline");
}

#[tokio::test]
async fn exchange_code_maps_google_token_payload() {
    let token_url = crate::test_support::serve_http(vec![(
        "200 OK",
        r#"{"access_token":"ya29.new","refresh_token":"1//refresh","expires_in":3599}"#.to_string(),
    )])
    .await;
    let token = exchange_code(&format!("{token_url}/token"), "code-123", "verifier")
        .await
        .unwrap();
    assert_eq!(token.access_token, "ya29.new");
    assert_eq!(token.refresh_token.as_deref(), Some("1//refresh"));
    assert_eq!(token.expires_in, Some(3599));
}

#[tokio::test]
async fn exchange_code_rejects_failed_or_incomplete_responses() {
    let cases = [
        (
            "400 Bad Request",
            r#"{"error":"invalid_grant"}"#,
            "Google token exchange failed",
        ),
        ("200 OK", r#"{"access_token":"ya29.x"}"#, "No refresh token received"),
        ("200 OK", "not json", "Failed to parse Google token response"),
    ];
    for (status, body, expected) in cases {
        let token_url = crate::test_support::serve_http(vec![(status, body.to_string())]).await;
        let err = exchange_code(&token_url, "code", "verifier").await.unwrap_err();
        assert!(err.to_string().contains(expected), "{status}: {err}");
    }
}

#[tokio::test]
async fn refresh_google_token_returns_refreshed_access_token() {
    let token_url = crate::test_support::serve_http(vec![(
        "200 OK",
        r#"{"access_token":"ya29.refreshed","expires_in":3599}"#.to_string(),
    )])
    .await;
    let token = refresh_google_token(&token_url, "1//old-refresh").await.unwrap();
    assert_eq!(token.access_token, "ya29.refreshed");
    assert_eq!(token.refresh_token, None);
}

#[tokio::test]
async fn fetch_user_email_reads_email_only_from_successful_json() {
    let cases = [
        ("200 OK", r#"{"email":"dev@example.com"}"#, Some("dev@example.com")),
        ("200 OK", r#"{"name":"no email here"}"#, None),
        ("403 Forbidden", r#"{"email":"dev@example.com"}"#, None),
        ("200 OK", "<html>", None),
    ];
    for (status, body, expected) in cases {
        let userinfo_url = crate::test_support::serve_http(vec![(status, body.to_string())]).await;
        assert_eq!(
            fetch_user_email(&userinfo_url, "ya29.token").await.as_deref(),
            expected,
            "{status} {body}"
        );
    }
}
