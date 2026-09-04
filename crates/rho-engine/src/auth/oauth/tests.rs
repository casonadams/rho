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
    assert_eq!(extract_chatgpt_account_id(""), None);
    assert_eq!(extract_chatgpt_account_id("not-a-jwt"), None);
    assert_eq!(extract_chatgpt_account_id("single.dot"), None);
    assert_eq!(extract_chatgpt_account_id("header.invalid!base64.signature"), None);
    assert_eq!(extract_chatgpt_account_id("header.bm90LWpzb24=.signature"), None);
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
}

#[tokio::test]
async fn test_refresh_oauth_token_unsupported_provider_passthrough() {
    let cred = StoredCredential::oauth("access-token".to_string(), Some("refresh-token".to_string()), None);
    let refreshed = refresh_oauth_token(ProviderId::Local, &cred).await.unwrap();
    assert_eq!(refreshed, cred);
}

#[test]
fn test_openrouter_build_auth_url() {
    let url_with_cb = openrouter::build_auth_url(Some("http://localhost:1234/callback"), "challenge123");
    assert!(url_with_cb.starts_with(openrouter::OPENROUTER_AUTH_URL));
    assert!(url_with_cb.contains("callback_url=http://localhost:1234/callback"));
    assert!(url_with_cb.contains("code_challenge=challenge123"));
    assert!(url_with_cb.contains("code_challenge_method=S256"));
    assert!(url_with_cb.contains("key_label=rho"));

    let url_headless = openrouter::build_auth_url(None, "challenge456");
    assert!(!url_headless.contains("callback_url="));
    assert!(url_headless.contains("code_challenge=challenge456"));
    assert!(url_headless.contains("key_label=rho"));
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
