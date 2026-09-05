use crate::auth::claude::*;
use async_trait::async_trait;
use rho_harness_core::auth::{DeviceCodeInfo, OAuthLoginCallbacks, SelectOption, StoredCredential};
use rho_harness_core::error::Result;

struct MockCallbacks {
    prompt_response: String,
    select_response: Option<String>,
}

#[async_trait]
impl OAuthLoginCallbacks for MockCallbacks {
    async fn on_auth_url(&self, _url: &str, _instructions: Option<&str>) -> Result<()> {
        Ok(())
    }
    async fn on_device_code(&self, _info: &DeviceCodeInfo<'_>) -> Result<()> {
        Ok(())
    }
    async fn on_prompt(&self, _message: &str, _secret: bool) -> Result<String> {
        Ok(self.prompt_response.clone())
    }
    async fn on_select(&self, _message: &str, _options: &[SelectOption]) -> Result<Option<String>> {
        Ok(self.select_response.clone())
    }
    async fn on_progress(&self, _message: &str) -> Result<()> {
        Ok(())
    }
}

#[test]
fn test_build_authorize_url() {
    let url = build_authorize_url("http://localhost:51122/callback", "challenge123", "state456");
    assert!(url.starts_with(AUTHORIZE_URL));
    assert!(url.contains("client_id=9d1c250a-e61b-44d9-88ed-5944d1962f5e"));
    assert!(
        url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A51122%2Fcallback")
            || url.contains("redirect_uri=http://localhost:51122/callback")
    );
    assert!(url.contains("code_challenge=challenge123"));
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains("state=state456"));
    assert!(url.contains("code=true"));
}

#[test]
fn test_parse_auth_code_and_state_query_params() {
    let input = "https://platform.claude.com/oauth/code/callback?code=abc-123&state=xyz-789";
    let (code, state) = parse_auth_code_and_state(input);
    assert_eq!(code, "abc-123");
    assert_eq!(state.as_deref(), Some("xyz-789"));
}

#[test]
fn test_parse_auth_code_and_state_fragment() {
    let input = "auth_code_value#state_token_value";
    let (code, state) = parse_auth_code_and_state(input);
    assert_eq!(code, "auth_code_value");
    assert_eq!(state.as_deref(), Some("state_token_value"));
}

#[test]
fn test_parse_auth_code_and_state_plain_code() {
    let input = "  simple_auth_code  ";
    let (code, state) = parse_auth_code_and_state(input);
    assert_eq!(code, "simple_auth_code");
    assert_eq!(state, None);
}

#[tokio::test]
async fn test_refresh_credential_missing_refresh_token() {
    let cred = StoredCredential::oauth("access".to_string(), None, None);
    let result = refresh_credential(&cred).await;
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("has expired and has no refresh token"));
}

#[tokio::test]
async fn test_refresh_credential_api_key_errors() {
    let cred = StoredCredential::api_key("sk-ant-test");
    let result = refresh_credential(&cred).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_perform_login_accept_local_credentials() {
    let callbacks = MockCallbacks {
        prompt_response: String::new(),
        select_response: Some("import".to_string()),
    };

    if detect_local_claude_credentials().is_some() {
        let cred = perform_login(&callbacks).await.unwrap();
        assert!(matches!(cred, StoredCredential::OAuth { .. }));
    }
}
