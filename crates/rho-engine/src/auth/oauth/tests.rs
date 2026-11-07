use super::*;
use crate::auth::loopback::LoopbackServer;
use crate::test_support::serve_http;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rho_harness_core::auth::{DeviceCodeInfo, OAuthLoginCallbacks, SelectOption, StoredCredential};
use rho_harness_core::provider::ProviderId;

struct DummyCallbacks;

#[async_trait::async_trait]
impl OAuthLoginCallbacks for DummyCallbacks {
    async fn on_auth_url(&self, _url: &str, _instructions: Option<&str>) -> Result<()> {
        Ok(())
    }
    async fn on_device_code(&self, _info: &DeviceCodeInfo<'_>) -> Result<()> {
        Ok(())
    }
    async fn on_prompt(&self, _message: &str, _secret: bool) -> Result<String> {
        Ok(String::new())
    }
    async fn on_select(&self, _message: &str, _options: &[SelectOption]) -> Result<Option<String>> {
        Ok(None)
    }
    async fn on_progress(&self, _message: &str) -> Result<()> {
        Ok(())
    }
}

#[test]
fn test_extract_chatgpt_account_id_valid() {
    let payload = serde_json::json!({
        "https://api.openai.com/auth": {
            "chatgpt_account_id": "org-test12345"
        },
        "sub": "user_xyz"
    });
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string());
    let jwt = format!("eyJhbGciOiJSUzI1NiJ9.{payload_b64}.signature");

    let account_id = extract_chatgpt_account_id(&jwt);
    assert_eq!(account_id.as_deref(), Some("org-test12345"));
}

#[test]
fn test_extract_chatgpt_account_id_standard_padded() {
    use base64::engine::general_purpose::STANDARD;
    let payload = serde_json::json!({
        "https://api.openai.com/auth": {
            "chatgpt_account_id": "acc-standard"
        }
    });
    let payload_b64 = STANDARD.encode(payload.to_string());
    let jwt = format!("header.{payload_b64}.signature");

    let account_id = extract_chatgpt_account_id(&jwt);
    assert_eq!(account_id.as_deref(), Some("acc-standard"));
}

#[test]
fn test_extract_chatgpt_account_id_missing_claim() {
    let payload = serde_json::json!({
        "sub": "user_xyz",
        "email": "user@example.com"
    });
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string());
    let jwt = format!("header.{payload_b64}.signature");

    assert_eq!(extract_chatgpt_account_id(&jwt), None);
}

#[test]
fn test_extract_chatgpt_account_id_malformed_jwt() {
    for input in [
        "",
        "not-a-jwt",
        "single.dot",
        "header.invalid!base64.signature",
        "header.bm90LWpzb24=.signature",
    ] {
        assert_eq!(extract_chatgpt_account_id(input), None);
    }
}

#[tokio::test]
async fn test_perform_oauth_login_unsupported_provider() {
    let callbacks = DummyCallbacks;
    let result = perform_oauth_login(ProviderId::Anthropic, &callbacks).await;
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("OAuth login is not supported for provider"));
}

#[tokio::test]
async fn test_refresh_oauth_token_api_key_passthrough() {
    let cred = StoredCredential::api_key("sk-test-key-123");
    let refreshed = refresh_oauth_token(ProviderId::ChatGpt, &cred).await.unwrap();
    assert_eq!(refreshed, cred);
}

#[tokio::test]
async fn test_refresh_oauth_token_missing_refresh_token() {
    let cred = StoredCredential::oauth("access-token".to_string(), None, None);
    let result = refresh_oauth_token(ProviderId::ChatGpt, &cred).await;
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("has expired and has no refresh token"));

    let result_claude = refresh_oauth_token(ProviderId::ClaudeCode, &cred).await;
    assert!(result_claude.is_err());
}

#[tokio::test]
async fn test_refresh_oauth_token_unsupported_provider_passthrough() {
    let cred = StoredCredential::oauth("access-token".to_string(), Some("refresh-token".to_string()), None);
    let refreshed = refresh_oauth_token(ProviderId::Local, &cred).await.unwrap();
    assert_eq!(refreshed, cred);
}

#[test]
fn test_openrouter_build_auth_url_with_callback() {
    let url = openrouter::build_auth_url(Some("http://localhost:1234/callback"), "challenge123");
    assert!(url.starts_with(openrouter::OPENROUTER_AUTH_URL));
    for fragment in [
        "callback_url=http://localhost:1234/callback",
        "code_challenge=challenge123",
        "code_challenge_method=S256",
        "key_label=rho",
    ] {
        assert!(url.contains(fragment));
    }
}

#[test]
fn test_openrouter_build_auth_url_headless() {
    let url = openrouter::build_auth_url(None, "challenge456");
    assert!(!url.contains("callback_url="));
    assert!(url.contains("code_challenge=challenge456") && url.contains("key_label=rho"));
}

#[test]
fn test_openrouter_build_exchange_body() {
    let body = openrouter::build_exchange_body("code123", "verifier456");
    assert_eq!(body.get("code"), Some(&"code123"));
    assert_eq!(body.get("code_verifier"), Some(&"verifier456"));
    assert_eq!(body.get("code_challenge_method"), Some(&"S256"));
}

#[test]
fn test_openrouter_parse_key_response() {
    let json_valid = r#"{"key": "sk-or-v1-abcdef123456"}"#;
    let key = openrouter::parse_key_response(json_valid).unwrap();
    assert_eq!(key, "sk-or-v1-abcdef123456");

    let json_empty_key = r#"{"key": "   "}"#;
    assert!(openrouter::parse_key_response(json_empty_key).is_err());

    let json_invalid = r#"{"error": "invalid_grant"}"#;
    assert!(openrouter::parse_key_response(json_invalid).is_err());
}

#[test]
fn test_chatgpt_build_auth_url() {
    let raw = chatgpt::build_openai_auth_url("challenge_abc", "state_123");
    assert!(!raw.contains(' '), "authorize URL must be percent-encoded: {raw}");
    let url = url::Url::parse(&raw).unwrap();
    assert_eq!(
        url.as_str().split('?').next(),
        Some("https://auth.openai.com/oauth/authorize")
    );
    let query: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();
    assert_eq!(query["client_id"], "app_EMoamEEZ73f0CkXaXp7hrann");
    assert_eq!(query["redirect_uri"], "http://localhost:1455/auth/callback");
    assert_eq!(query["scope"], "openid profile email offline_access");
    assert_eq!(query["code_challenge"], "challenge_abc");
    assert_eq!(query["state"], "state_123");
    assert_eq!(query["originator"], "rho");
}

#[test]
fn test_validate_callback_outcomes() {
    use crate::auth::loopback::CallbackParams;
    let params =
        |code: Option<&str>, state: Option<&str>, error: Option<&str>, description: Option<&str>| CallbackParams {
            code: code.map(str::to_string),
            state: state.map(str::to_string),
            error: error.map(str::to_string),
            error_description: description.map(str::to_string),
        };
    let accepted = validate_callback(
        params(Some("auth_code_xyz"), Some("state_123"), None, None),
        "state_123",
    );
    assert_eq!(accepted.unwrap(), "auth_code_xyz");

    let rejected = [
        (
            params(None, None, Some("access_denied"), Some("User denied access")),
            "access_denied User denied access",
        ),
        (params(None, None, None, None), "No authorization code received"),
        (
            params(Some("auth_code_xyz"), Some("attacker_state"), None, None),
            "OAuth state mismatch",
        ),
    ];
    for (callback, expected) in rejected {
        let err = validate_callback(callback, "state_123").unwrap_err();
        assert!(err.to_string().contains(expected), "{err} should contain {expected}");
    }
}

#[test]
fn test_chatgpt_build_openai_credential() {
    let payload = serde_json::json!({
        "https://api.openai.com/auth": {
            "chatgpt_account_id": "org-chatgpt-123"
        },
        "sub": "user_123"
    });
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string());
    let token = chatgpt::TokenResponse {
        access_token: format!("header.{payload_b64}.sig"),
        refresh_token: Some("rt_xyz".to_string()),
        expires_in: Some(3600),
    };
    let cred = chatgpt::build_openai_credential(token);
    let StoredCredential::OAuth {
        access_token,
        refresh_token,
        expires_at_ms,
        account_id,
        ..
    } = cred
    else {
        panic!("expected OAuth credential");
    };
    assert_eq!(account_id.as_deref(), Some("org-chatgpt-123"));
    assert_eq!(access_token, format!("header.{payload_b64}.sig"));
    assert_eq!(refresh_token.as_deref(), Some("rt_xyz"));
    assert!(expires_at_ms.is_some());
}

type CallbackQuery = fn(&str) -> String;

struct LoopbackClient {
    port: u16,
    callback_query: CallbackQuery,
}

#[async_trait::async_trait]
impl OAuthLoginCallbacks for LoopbackClient {
    async fn on_auth_url(&self, url: &str, _instructions: Option<&str>) -> Result<()> {
        let state = url::Url::parse(url)
            .unwrap()
            .query_pairs()
            .find(|(key, _)| key == "state")
            .unwrap()
            .1
            .into_owned();
        let request = format!(
            "GET /callback?{} HTTP/1.1\r\nHost: localhost\r\n\r\n",
            (self.callback_query)(&state)
        );
        let port = self.port;
        tokio::spawn(async move {
            let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            tokio::io::AsyncWriteExt::write_all(&mut socket, request.as_bytes())
                .await
                .unwrap();
        });
        Ok(())
    }
    async fn on_device_code(&self, _info: &DeviceCodeInfo<'_>) -> Result<()> {
        Ok(())
    }
    async fn on_prompt(&self, _message: &str, _secret: bool) -> Result<String> {
        Ok(String::new())
    }
    async fn on_select(&self, _message: &str, _options: &[SelectOption]) -> Result<Option<String>> {
        Ok(None)
    }
    async fn on_progress(&self, _message: &str) -> Result<()> {
        Ok(())
    }
}

fn test_auth_url(challenge: &str, state: &str) -> String {
    format!("https://auth.example.test/authorize?code_challenge={challenge}&state={state}")
}

#[tokio::test]
async fn loopback_login_redeems_only_callbacks_with_matching_state() {
    fn valid(state: &str) -> String {
        format!("code=auth-code-1&state={state}")
    }
    fn forged(_state: &str) -> String {
        "code=auth-code-1&state=forged".to_string()
    }
    fn denied(_state: &str) -> String {
        "error=access_denied&error_description=User%20denied".to_string()
    }
    let cases: [(CallbackQuery, Option<&str>); 3] = [
        (valid, None),
        (forged, Some("state mismatch")),
        (denied, Some("access_denied")),
    ];
    for (index, (callback_query, expected_err)) in cases.into_iter().enumerate() {
        let client = LoopbackClient {
            port: 23_150 + index as u16,
            callback_query,
        };
        let prompt = LoginPrompt {
            port: client.port,
            callback_timeout: Duration::from_secs(5),
            hint: "Finish sign-in.",
            progress: "Waiting for authorization...",
        };
        let result = loopback_login(&client, &prompt, test_auth_url, |grant| async move {
            Ok::<_, AppError>((grant.code, grant.verifier))
        })
        .await;
        match expected_err {
            None => {
                let (code, verifier) = result.unwrap();
                assert_eq!(code, "auth-code-1");
                assert!(verifier.len() >= 43, "PKCE verifier too short: {verifier}");
            }
            Some(expected) => {
                let err = result.expect_err("callback must be rejected");
                assert!(err.to_string().contains(expected), "{err} should contain {expected}");
            }
        }
    }
}

#[tokio::test]
async fn bind_for_login_reports_busy_port_with_recovery_hint() {
    let busy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = busy.local_addr().unwrap().port();
    let err = LoopbackServer::bind_for_login(&DummyCallbacks, port, "https://auth.example.test", "hint", "progress")
        .await
        .err()
        .expect("busy port must fail");
    assert!(
        err.to_string()
            .contains(&format!("Ensure no other process is using port {port}"))
    );
}

#[tokio::test]
async fn exchange_openai_code_maps_token_payload() {
    let token_url = serve_http(vec![(
        "200 OK",
        r#"{"access_token":"oai-access","refresh_token":"oai-refresh","expires_in":3600}"#.to_string(),
    )])
    .await;
    let token = chatgpt::exchange_openai_code(&token_url, "code-1", "verifier-1")
        .await
        .unwrap();
    assert_eq!(token.access_token, "oai-access");
    assert_eq!(token.refresh_token.as_deref(), Some("oai-refresh"));
    assert_eq!(token.expires_in, Some(3600));
}

#[tokio::test]
async fn openai_token_requests_surface_failures_by_operation() {
    let cases = [
        (
            "400 Bad Request",
            r#"{"error":"invalid_grant"}"#,
            "OpenAI token exchange failed (400",
        ),
        ("200 OK", "not json", "Failed to parse OpenAI token exchange response"),
    ];
    for (status, body, expected) in cases {
        let token_url = serve_http(vec![(status, body.to_string())]).await;
        let err = chatgpt::exchange_openai_code(&token_url, "code-1", "verifier-1")
            .await
            .unwrap_err();
        assert!(err.to_string().contains(expected), "{status}: {err}");
    }
    let token_url = serve_http(vec![("401 Unauthorized", r#"{"error":"revoked"}"#.to_string())]).await;
    let err = chatgpt::post_refresh_request(&token_url, "oai-refresh-stale")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("OpenAI token refresh failed (401"), "{err}");
}

#[tokio::test]
async fn post_refresh_request_returns_token_without_rotated_refresh() {
    let token_url = serve_http(vec![(
        "200 OK",
        r#"{"access_token":"oai-new","expires_in":3600}"#.to_string(),
    )])
    .await;
    let token = chatgpt::post_refresh_request(&token_url, "oai-refresh").await.unwrap();
    assert_eq!(token.access_token, "oai-new");
    assert_eq!(token.refresh_token, None);
}

#[tokio::test]
async fn exchange_openrouter_key_returns_key_and_rejects_bad_responses() {
    let keys_url = serve_http(vec![("200 OK", r#"{"key":"sk-or-v1-abc"}"#.to_string())]).await;
    assert_eq!(
        openrouter::exchange_openrouter_key(&keys_url, "code-1", "verifier-1")
            .await
            .unwrap(),
        "sk-or-v1-abc"
    );
    let cases = [
        (
            "401 Unauthorized",
            r#"{"error":"bad"}"#,
            "OpenRouter key exchange failed (401",
        ),
        ("200 OK", r#"{"key":"   "}"#, "OpenRouter returned an empty API key"),
        ("200 OK", "oops", "Failed to parse OpenRouter key response"),
    ];
    for (status, body, expected) in cases {
        let keys_url = serve_http(vec![(status, body.to_string())]).await;
        let err = openrouter::exchange_openrouter_key(&keys_url, "code-1", "verifier-1")
            .await
            .unwrap_err();
        assert!(err.to_string().contains(expected), "{status}: {err}");
    }
}
