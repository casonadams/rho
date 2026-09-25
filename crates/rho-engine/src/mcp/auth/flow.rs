use crate::auth::loopback::{CallbackParams, LoopbackServer};
use crate::auth::pkce::{PkceChallenge, generate_state};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use rho_harness_core::auth::{OAuthLoginCallbacks, StoredCredential};
use rho_harness_core::error::{AppError, Result};
use serde::Deserialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_MCP_CLIENT_ID: &str = "rho-mcp-client";

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct TokenResponse {
    pub(crate) access_token: String,
    #[serde(default)]
    pub(crate) refresh_token: Option<String>,
    #[serde(default)]
    pub(crate) expires_in: Option<u64>,
}

pub fn build_mcp_auth_url(
    auth_endpoint: &str,
    client_id: &str,
    redirect_uri: &str,
    code_challenge: &str,
    state: &str,
    scope: Option<&str>,
) -> String {
    let enc_redirect = utf8_percent_encode(redirect_uri, NON_ALPHANUMERIC);
    let mut auth_url = format!(
        "{auth_endpoint}?response_type=code&client_id={client_id}&redirect_uri={enc_redirect}&code_challenge={code_challenge}&code_challenge_method=S256&state={state}"
    );
    if let Some(s) = scope {
        auth_url.push_str("&scope=");
        auth_url.push_str(&utf8_percent_encode(s, NON_ALPHANUMERIC).to_string());
    }
    auth_url
}

pub(crate) fn validate_mcp_callback(params: CallbackParams, expected_state: &str) -> Result<String> {
    if let Some(err) = params.error {
        return Err(AppError::Auth(format!("OAuth failed: {err}")));
    }

    let code = params
        .code
        .ok_or_else(|| AppError::Auth("No authorization code received".to_string()))?;

    if params.state.as_deref() != Some(expected_state) {
        return Err(AppError::Auth("OAuth state mismatch".to_string()));
    }

    Ok(code)
}

pub(crate) fn build_mcp_token_form<'a>(
    code: &'a str,
    redirect_uri: &'a str,
    client_id: &'a str,
    code_verifier: &'a str,
) -> [(&'static str, &'a str); 5] {
    [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", client_id),
        ("code_verifier", code_verifier),
    ]
}

pub(crate) async fn exchange_mcp_code(
    client: &reqwest::Client,
    token_endpoint: &str,
    form: &[(&'static str, &str)],
) -> Result<TokenResponse> {
    let res = client
        .post(token_endpoint)
        .form(form)
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Token exchange request failed: {e}")))?;

    if !res.status().is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(AppError::Auth(format!("Token exchange failed: {body}")));
    }

    res.json()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to parse token response: {e}")))
}

pub(crate) fn build_mcp_credential(token_data: TokenResponse, now_ms: i64) -> StoredCredential {
    let expires_at_ms = token_data.expires_in.map(|secs| now_ms + (secs as i64) * 1000);
    StoredCredential::oauth(token_data.access_token, token_data.refresh_token, expires_at_ms)
}

pub async fn execute_mcp_pkce_flow(
    server_name: &str,
    auth_endpoint: &str,
    token_endpoint: &str,
    scope: Option<&str>,
    client_id: Option<&str>,
    callbacks: &dyn OAuthLoginCallbacks,
) -> Result<StoredCredential> {
    let pkce = PkceChallenge::generate();
    let state = generate_state();
    let loopback = LoopbackServer::bind().await?;
    let redirect_uri = loopback.redirect_uri("/callback");
    let cid = client_id.unwrap_or(DEFAULT_MCP_CLIENT_ID);

    let auth_url = build_mcp_auth_url(auth_endpoint, cid, &redirect_uri, &pkce.challenge, &state, scope);

    callbacks
        .on_auth_url(
            &auth_url,
            Some(&format!(
                "Complete authorization in your browser for MCP server '{server_name}'"
            )),
        )
        .await?;

    callbacks.on_progress("Waiting for browser authentication...").await?;

    let params = loopback.wait_for_callback(Duration::from_secs(120)).await?;
    let code = validate_mcp_callback(params, &state)?;

    callbacks
        .on_progress("Exchanging authorization code for token...")
        .await?;

    let form = build_mcp_token_form(&code, &redirect_uri, cid, &pkce.verifier);
    let client = crate::auth::http::http_client();
    let token_data = exchange_mcp_code(client, token_endpoint, &form).await?;

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    Ok(build_mcp_credential(token_data, now_ms))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_harness_core::auth::{DeviceCodeInfo, SelectOption};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    struct MockCallbacks {
        auto_callback: bool,
    }

    #[async_trait::async_trait]
    impl OAuthLoginCallbacks for MockCallbacks {
        async fn on_auth_url(&self, url: &str, _instructions: Option<&str>) -> Result<()> {
            if self.auto_callback {
                let parsed = url::Url::parse(url).map_err(|e| AppError::Auth(e.to_string()))?;
                let mut redirect = None;
                let mut state = None;
                for (k, v) in parsed.query_pairs() {
                    if k == "redirect_uri" {
                        redirect = Some(v.into_owned());
                    } else if k == "state" {
                        state = Some(v.into_owned());
                    }
                }
                let target = format!("{}?code=mock_auth_code_123&state={}", redirect.unwrap(), state.unwrap());
                tokio::spawn(async move {
                    let client = crate::auth::http::http_client();
                    let _ = client.get(&target).send().await;
                });
            }
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
    fn test_build_mcp_auth_url() {
        let url_no_scope = build_mcp_auth_url(
            "https://auth.example.com/oauth",
            "client_1",
            "http://localhost:1234/callback",
            "challenge_abc",
            "state_xyz",
            None,
        );
        assert!(url_no_scope.starts_with("https://auth.example.com/oauth?response_type=code"));
        assert!(url_no_scope.contains("client_id=client_1"));
        assert!(url_no_scope.contains("redirect_uri=http%3A%2F%2Flocalhost%3A1234%2Fcallback"));
        assert!(url_no_scope.contains("code_challenge=challenge_abc"));
        assert!(url_no_scope.contains("code_challenge_method=S256"));
        assert!(url_no_scope.contains("state=state_xyz"));
        assert!(!url_no_scope.contains("scope="));

        let url_with_scope = build_mcp_auth_url(
            "https://auth.example.com/oauth",
            "client_1",
            "http://localhost:1234/callback",
            "challenge_abc",
            "state_xyz",
            Some("tools:read tools:write"),
        );
        assert!(url_with_scope.contains("&scope=tools%3Aread%20tools%3Awrite"));
    }

    #[test]
    fn test_validate_mcp_callback_success() {
        let params = CallbackParams {
            code: Some("auth_code_123".to_string()),
            state: Some("state_abc".to_string()),
            error: None,
            error_description: None,
        };
        let code = validate_mcp_callback(params, "state_abc").unwrap();
        assert_eq!(code, "auth_code_123");
    }

    #[test]
    fn test_validate_mcp_callback_error() {
        let params = CallbackParams {
            code: None,
            state: None,
            error: Some("access_denied".to_string()),
            error_description: Some("user cancelled".to_string()),
        };
        let err = validate_mcp_callback(params, "state_abc").unwrap_err();
        assert!(err.to_string().contains("access_denied"));
    }

    #[test]
    fn test_validate_mcp_callback_missing_code() {
        let params = CallbackParams {
            code: None,
            state: Some("state_abc".to_string()),
            error: None,
            error_description: None,
        };
        let err = validate_mcp_callback(params, "state_abc").unwrap_err();
        assert!(err.to_string().contains("No authorization code received"));
    }

    #[test]
    fn test_validate_mcp_callback_state_mismatch() {
        let params = CallbackParams {
            code: Some("auth_code_123".to_string()),
            state: Some("wrong_state".to_string()),
            error: None,
            error_description: None,
        };
        let err = validate_mcp_callback(params, "state_abc").unwrap_err();
        assert!(err.to_string().contains("OAuth state mismatch"));
    }

    #[test]
    fn test_build_mcp_token_form() {
        let form = build_mcp_token_form("c1", "http://callback", "cid", "verifier");
        assert_eq!(form[0], ("grant_type", "authorization_code"));
        assert_eq!(form[1], ("code", "c1"));
        assert_eq!(form[2], ("redirect_uri", "http://callback"));
        assert_eq!(form[3], ("client_id", "cid"));
        assert_eq!(form[4], ("code_verifier", "verifier"));
    }

    #[test]
    fn test_build_mcp_credential() {
        let token_data = TokenResponse {
            access_token: "at_123".to_string(),
            refresh_token: Some("rt_456".to_string()),
            expires_in: Some(3600),
        };
        let cred = build_mcp_credential(token_data, 1_000_000);
        let StoredCredential::OAuth {
            access_token,
            refresh_token,
            expires_at_ms,
            ..
        } = cred
        else {
            panic!("Expected OAuth credential");
        };
        assert_eq!(access_token, "at_123");
        assert_eq!(refresh_token.as_deref(), Some("rt_456"));
        assert_eq!(expires_at_ms, Some(1_000_000 + 3600 * 1000));

        let token_data_no_expire = TokenResponse {
            access_token: "at_no_exp".to_string(),
            refresh_token: None,
            expires_in: None,
        };
        let cred_no_exp = build_mcp_credential(token_data_no_expire, 1_000_000);
        if let StoredCredential::OAuth { expires_at_ms, .. } = cred_no_exp {
            assert_eq!(expires_at_ms, None);
        } else {
            panic!("Expected OAuth credential");
        }
    }

    #[tokio::test]
    async fn test_exchange_mcp_code_success() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let token_endpoint = format!("http://{addr}/token");

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 1024];
            let _ = socket.read(&mut buf).await.unwrap();

            let resp_body =
                r#"{"access_token":"exchanged-token","refresh_token":"exchanged-refresh","expires_in":1800}"#;
            let http_resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                resp_body.len(),
                resp_body
            );
            socket.write_all(http_resp.as_bytes()).await.unwrap();
        });

        let form = build_mcp_token_form("code", "http://callback", "cid", "verifier");
        let client = crate::auth::http::http_client();
        let resp = exchange_mcp_code(client, &token_endpoint, &form).await.unwrap();
        assert_eq!(resp.access_token, "exchanged-token");
        assert_eq!(resp.refresh_token.as_deref(), Some("exchanged-refresh"));
        assert_eq!(resp.expires_in, Some(1800));
    }

    #[tokio::test]
    async fn test_exchange_mcp_code_http_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let token_endpoint = format!("http://{addr}/token");

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 1024];
            let _ = socket.read(&mut buf).await.unwrap();

            let resp_body = r#"{"error":"invalid_grant"}"#;
            let http_resp = format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                resp_body.len(),
                resp_body
            );
            socket.write_all(http_resp.as_bytes()).await.unwrap();
        });

        let form = build_mcp_token_form("bad_code", "http://callback", "cid", "verifier");
        let client = crate::auth::http::http_client();
        let err = exchange_mcp_code(client, &token_endpoint, &form).await.unwrap_err();
        assert!(err.to_string().contains("Token exchange failed"));
    }

    #[tokio::test]
    async fn test_exchange_mcp_code_invalid_json() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let token_endpoint = format!("http://{addr}/token");

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 1024];
            let _ = socket.read(&mut buf).await.unwrap();

            let resp_body = "not json";
            let http_resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                resp_body.len(),
                resp_body
            );
            socket.write_all(http_resp.as_bytes()).await.unwrap();
        });

        let form = build_mcp_token_form("code", "http://callback", "cid", "verifier");
        let client = crate::auth::http::http_client();
        let err = exchange_mcp_code(client, &token_endpoint, &form).await.unwrap_err();
        assert!(err.to_string().contains("Failed to parse token response"));
    }

    #[tokio::test]
    async fn test_execute_mcp_pkce_flow_full() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let token_endpoint = format!("http://{addr}/token");

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 1024];
            let _ = socket.read(&mut buf).await.unwrap();

            let resp_body =
                r#"{"access_token":"flow-access-token","refresh_token":"flow-refresh-token","expires_in":7200}"#;
            let http_resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                resp_body.len(),
                resp_body
            );
            socket.write_all(http_resp.as_bytes()).await.unwrap();
        });

        let callbacks = MockCallbacks { auto_callback: true };
        let cred = execute_mcp_pkce_flow(
            "test-server",
            "http://auth.example.com/authorize",
            &token_endpoint,
            Some("tools:read"),
            Some("custom-client-id"),
            &callbacks,
        )
        .await
        .unwrap();

        assert_eq!(cred.raw_secret(), "flow-access-token");
        if let StoredCredential::OAuth {
            refresh_token,
            expires_at_ms,
            ..
        } = cred
        {
            assert_eq!(refresh_token.as_deref(), Some("flow-refresh-token"));
            assert!(expires_at_ms.is_some());
        } else {
            panic!("Expected OAuth credential");
        }
    }

    #[tokio::test]
    async fn test_execute_mcp_pkce_flow_default_client() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let token_endpoint = format!("http://{addr}/token");

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 1024];
            let _ = socket.read(&mut buf).await.unwrap();

            let resp_body = r#"{"access_token":"default-token"}"#;
            let http_resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                resp_body.len(),
                resp_body
            );
            socket.write_all(http_resp.as_bytes()).await.unwrap();
        });

        let callbacks = MockCallbacks { auto_callback: true };
        let cred = execute_mcp_pkce_flow(
            "test-server",
            "http://auth.example.com/authorize",
            &token_endpoint,
            None,
            None,
            &callbacks,
        )
        .await
        .unwrap();

        assert_eq!(cred.raw_secret(), "default-token");
    }
}
