use super::*;
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
    let url = chatgpt::build_openai_auth_url("challenge_abc", "state_123");
    assert!(url.starts_with("https://auth.openai.com/oauth/authorize"));
    for fragment in [
        "client_id=app_EMoamEEZ73f0CkXaXp7hrann",
        "redirect_uri=http://localhost:1455/auth/callback",
        "code_challenge=challenge_abc",
        "state=state_123",
        "originator=rho",
    ] {
        assert!(url.contains(fragment));
    }
}

#[test]
fn test_chatgpt_validate_callback_success() {
    let callback = crate::auth::loopback::CallbackParams {
        code: Some("auth_code_xyz".to_string()),
        state: Some("state_123".to_string()),
        error: None,
        error_description: None,
    };
    let code = chatgpt::validate_callback(callback).unwrap();
    assert_eq!(code, "auth_code_xyz");
}

#[test]
fn test_chatgpt_validate_callback_error() {
    let callback = crate::auth::loopback::CallbackParams {
        code: None,
        state: None,
        error: Some("access_denied".to_string()),
        error_description: Some("User denied access".to_string()),
    };
    let err = chatgpt::validate_callback(callback).unwrap_err();
    assert!(err.to_string().contains("access_denied User denied access"));
}

#[test]
fn test_chatgpt_validate_callback_missing_code() {
    let callback = crate::auth::loopback::CallbackParams {
        code: None,
        state: None,
        error: None,
        error_description: None,
    };
    let err = chatgpt::validate_callback(callback).unwrap_err();
    assert!(err.to_string().contains("No authorization code received"));
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

async fn spawn_mock_http_response(status_line: &str, body: &str) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let status = status_line.to_string();
    let body = body.to_string();
    let handle = tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buf = [0u8; 1024];
            let _ = socket.read(&mut buf).await;
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.flush().await;
        }
    });
    (format!("http://{addr}"), handle)
}

#[test]
fn test_copilot_parse_poll_payload() {
    let val_success = serde_json::json!({"access_token": "ghu_abc123"});
    assert_eq!(
        copilot::parse_poll_payload(&val_success).unwrap().unwrap(),
        "ghu_abc123"
    );

    let val_pending = serde_json::json!({"error": "authorization_pending"});
    assert!(copilot::parse_poll_payload(&val_pending).is_none());

    let val_error = serde_json::json!({"error": "expired_token"});
    let err = copilot::parse_poll_payload(&val_error).unwrap().unwrap_err();
    assert!(err.to_string().contains("expired_token"));

    let val_empty = serde_json::json!({});
    assert!(copilot::parse_poll_payload(&val_empty).is_none());
}

#[test]
fn test_copilot_build_credential_and_form() {
    let cred = copilot::build_copilot_credential("ghu_token".to_string(), "gho_token".to_string(), 1234);
    let StoredCredential::OAuth {
        access_token,
        refresh_token,
        expires_at_ms,
        ..
    } = cred
    else {
        panic!("expected OAuth credential");
    };
    assert_eq!(access_token, "ghu_token");
    assert_eq!(refresh_token.as_deref(), Some("gho_token"));
    assert_eq!(expires_at_ms, Some(1234 * 1000));

    let form = copilot::build_device_token_form("device_code_xyz");
    assert_eq!(form[0], ("client_id", "Iv1.b507a08c87ecfe81"));
    assert_eq!(form[1], ("device_code", "device_code_xyz"));
    assert_eq!(form[2], ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"));
}

#[tokio::test]
async fn test_copilot_check_poll_response_success() {
    let (url, _handle) = spawn_mock_http_response("200 OK", r#"{"access_token": "ghu_test_token"}"#).await;
    let client = http_client();
    let resp = client.get(&url).send().await.unwrap();
    let result = copilot::check_poll_response(resp).await;
    assert_eq!(result.unwrap().unwrap(), "ghu_test_token");
}

#[tokio::test]
async fn test_copilot_check_poll_response_non_success() {
    let (url, _handle) = spawn_mock_http_response("400 Bad Request", r#"{"error": "bad_request"}"#).await;
    let client = http_client();
    let resp = client.get(&url).send().await.unwrap();
    let result = copilot::check_poll_response(resp).await;
    assert!(result.is_none());
}

#[tokio::test]
async fn test_copilot_check_poll_response_pending() {
    let (url, _handle) = spawn_mock_http_response("200 OK", r#"{"error": "authorization_pending"}"#).await;
    let client = http_client();
    let resp = client.get(&url).send().await.unwrap();
    let result = copilot::check_poll_response(resp).await;
    assert!(result.is_none());
}

#[tokio::test]
async fn test_copilot_check_poll_response_error() {
    let (url, _handle) = spawn_mock_http_response("200 OK", r#"{"error": "slow_down"}"#).await;
    let client = http_client();
    let resp = client.get(&url).send().await.unwrap();
    let result = copilot::check_poll_response(resp).await;
    let err = result.unwrap().unwrap_err();
    assert!(err.to_string().contains("slow_down"));
}

#[tokio::test]
async fn test_copilot_check_poll_response_invalid_json() {
    let (url, _handle) = spawn_mock_http_response("200 OK", "not json at all").await;
    let client = http_client();
    let resp = client.get(&url).send().await.unwrap();
    let result = copilot::check_poll_response(resp).await;
    assert!(result.is_none());
}
