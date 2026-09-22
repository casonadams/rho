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
    let url = build_authorize_url(
        "https://platform.claude.com/oauth/code/callback",
        "challenge123",
        "state456",
    );
    assert!(url.starts_with(AUTHORIZE_URL));
    let fragments = [
        "client_id=9d1c250a-e61b-44d9-88ed-5944d1962f5e",
        "code_challenge=challenge123",
        "code_challenge_method=S256",
        "state=state456",
        "code=true",
        "redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback",
    ];
    for f in fragments {
        assert!(url.contains(f));
    }
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

#[test]
fn test_parse_auth_code_and_state_query_params_extra_and_missing() {
    let input = "https://platform.claude.com/oauth/code/callback?unrelated=foo&bare_flag&code=c1&state=s1&other=bar";
    let (code, state) = parse_auth_code_and_state(input);
    assert_eq!(code, "c1");
    assert_eq!(state.as_deref(), Some("s1"));

    let input_no_code = "https://platform.claude.com/oauth/code/callback?other=bar";
    let (code, state) = parse_auth_code_and_state(input_no_code);
    assert_eq!(code, input_no_code);
    assert_eq!(state, None);
}

#[tokio::test]
async fn test_acquire_auth_code_success_with_state() {
    let callbacks = MockCallbacks {
        prompt_response: "auth_code_123#state_xyz".to_string(),
        select_response: None,
    };
    let (code, redirect_uri) = acquire_auth_code(&callbacks, "challenge", "state_xyz").await.unwrap();
    assert_eq!(code, "auth_code_123");
    assert_eq!(redirect_uri, REDIRECT_URI);
}

#[tokio::test]
async fn test_acquire_auth_code_success_plain_code() {
    let callbacks = MockCallbacks {
        prompt_response: "just_the_code".to_string(),
        select_response: None,
    };
    let (code, redirect_uri) = acquire_auth_code(&callbacks, "challenge", "state_xyz").await.unwrap();
    assert_eq!(code, "just_the_code");
    assert_eq!(redirect_uri, REDIRECT_URI);
}

#[tokio::test]
async fn test_acquire_auth_code_empty_code_error() {
    let callbacks = MockCallbacks {
        prompt_response: "   ".to_string(),
        select_response: None,
    };
    let err = acquire_auth_code(&callbacks, "challenge", "state_xyz")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("cannot be empty"));
}

#[tokio::test]
async fn test_acquire_auth_code_state_mismatch_error() {
    let callbacks = MockCallbacks {
        prompt_response: "code_123#wrong_state".to_string(),
        select_response: None,
    };
    let err = acquire_auth_code(&callbacks, "challenge", "expected_state")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("OAuth state mismatch"));
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

#[tokio::test]
async fn test_perform_login_no_local_credentials_fails_on_empty_code() {
    let callbacks = MockCallbacks {
        prompt_response: "   ".to_string(),
        select_response: None,
    };
    let result = perform_login(&callbacks).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_confirm_import_local_credentials_cases() {
    use crate::auth::claude::confirm_import_local_credentials;

    let callbacks_none = MockCallbacks {
        prompt_response: String::new(),
        select_response: None,
    };
    let res = confirm_import_local_credentials(None, &callbacks_none).await.unwrap();
    assert!(res.is_none());

    let cred = StoredCredential::oauth("tok".to_string(), None, None);
    let callbacks_import = MockCallbacks {
        prompt_response: String::new(),
        select_response: Some("import".to_string()),
    };
    let res = confirm_import_local_credentials(Some(cred.clone()), &callbacks_import)
        .await
        .unwrap();
    assert_eq!(res.unwrap().raw_secret(), "tok");

    let callbacks_browser = MockCallbacks {
        prompt_response: String::new(),
        select_response: Some("browser".to_string()),
    };
    let res = confirm_import_local_credentials(Some(cred), &callbacks_browser)
        .await
        .unwrap();
    assert!(res.is_none());
}
