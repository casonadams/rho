pub mod discovery;
pub mod flow;

use crate::auth::AuthStore;
use discovery::{
    AuthorizationServerMetadata, extract_required_scope, extract_resource_metadata_url, fetch_auth_server_metadata,
    fetch_protected_resource_metadata,
};
use flow::execute_mcp_pkce_flow;
use rho_harness_core::auth::{OAuthLoginCallbacks, StoredCredential};
use rho_harness_core::config::McpServerConfig;
use rho_harness_core::error::{AppError, Result};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_MCP_CLIENT_ID: &str = "rho-mcp-client";

pub async fn refresh_mcp_token(
    token_endpoint: &str,
    client_id: Option<&str>,
    cred: &StoredCredential,
) -> Result<StoredCredential> {
    let StoredCredential::OAuth { refresh_token, .. } = cred else {
        return Err(AppError::Auth("Cannot refresh non-OAuth credential".to_string()));
    };

    let Some(refresh) = refresh_token else {
        return Err(AppError::Auth("No refresh token available".to_string()));
    };

    let mut form = HashMap::new();
    form.insert("grant_type", "refresh_token");
    form.insert("refresh_token", refresh.as_str());
    form.insert("client_id", client_id.unwrap_or(DEFAULT_MCP_CLIENT_ID));

    let client = crate::auth::http::http_client();
    let res = client
        .post(token_endpoint)
        .form(&form)
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Token refresh request failed: {e}")))?;

    if !res.status().is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(AppError::Auth(format!("Token refresh failed: {body}")));
    }

    #[derive(serde::Deserialize)]
    struct RefreshResponse {
        access_token: String,
        #[serde(default)]
        refresh_token: Option<String>,
        #[serde(default)]
        expires_in: Option<u64>,
    }

    let token_data: RefreshResponse = res
        .json()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to parse token response: {e}")))?;

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let expires_at_ms = token_data.expires_in.map(|secs| now_ms + (secs as i64) * 1000);

    let new_refresh = token_data.refresh_token.or_else(|| refresh_token.clone());

    Ok(StoredCredential::oauth(
        token_data.access_token,
        new_refresh,
        expires_at_ms,
    ))
}

fn resolve_probe_target(status: reqwest::StatusCode, www_auth: Option<&str>, url: &str) -> (String, Option<String>) {
    let base_url = url.trim_end_matches('/');
    if status == reqwest::StatusCode::UNAUTHORIZED {
        let auth_header = www_auth.unwrap_or("");
        let meta_target = extract_resource_metadata_url(auth_header)
            .unwrap_or_else(|| format!("{base_url}/.well-known/oauth-protected-resource"));
        let scope = extract_required_scope(auth_header);
        (meta_target, scope)
    } else {
        (format!("{base_url}/.well-known/oauth-protected-resource"), None)
    }
}

async fn probe_mcp_server(client: &'static reqwest::Client, url: &str) -> Result<(String, Option<String>)> {
    let res = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to probe MCP server at '{url}': {e}")))?;

    let www_auth = res
        .headers()
        .get(reqwest::header::WWW_AUTHENTICATE)
        .and_then(|v| v.to_str().ok());

    Ok(resolve_probe_target(res.status(), www_auth, url))
}

async fn discover_mcp_auth_metadata(
    client: &'static reqwest::Client,
    meta_target: &str,
) -> Result<AuthorizationServerMetadata> {
    let protected_meta = fetch_protected_resource_metadata(client, meta_target).await?;
    let auth_server = protected_meta
        .authorization_servers
        .first()
        .ok_or_else(|| AppError::Auth("No authorization servers advertised by MCP server".to_string()))?;

    fetch_auth_server_metadata(client, auth_server).await
}

async fn store_mcp_credential(
    auth_store: &mut AuthStore,
    server_name: &str,
    cred: StoredCredential,
) -> Result<StoredCredential> {
    let key = format!("mcp:{server_name}");
    auth_store.set_credential(&key, cred.clone())?;
    let _ = auth_store.save_async().await;
    Ok(cred)
}

pub async fn login_mcp_server(
    server_name: &str,
    server_config: &McpServerConfig,
    auth_store: &mut AuthStore,
    callbacks: &dyn OAuthLoginCallbacks,
) -> Result<StoredCredential> {
    let url = server_config
        .url
        .as_deref()
        .ok_or_else(|| AppError::Auth(format!("Server '{server_name}' has no URL configured for OAuth")))?;

    let client = crate::auth::http::http_client();
    let (meta_target, scope) = probe_mcp_server(client, url).await?;
    let auth_meta = discover_mcp_auth_metadata(client, &meta_target).await?;

    let cred = execute_mcp_pkce_flow(
        server_name,
        &auth_meta.authorization_endpoint,
        &auth_meta.token_endpoint,
        scope.as_deref(),
        None,
        callbacks,
    )
    .await?;

    store_mcp_credential(auth_store, server_name, cred).await
}

pub async fn get_valid_mcp_token(server_name: &str, auth_store: &mut AuthStore) -> Option<String> {
    let key = format!("mcp:{server_name}");
    auth_store.get_key(&key).await.ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_harness_core::auth::{DeviceCodeInfo, SelectOption};
    use tempfile::tempdir;
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
                let target = format!("{}?code=mock_code&state={}", redirect.unwrap(), state.unwrap());
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
        async fn on_progress(&self, _message: &str) -> Result<()> {
            Ok(())
        }
        async fn on_select(&self, _message: &str, _options: &[SelectOption]) -> Result<Option<String>> {
            Ok(None)
        }
    }

    #[test]
    fn test_resolve_probe_target_unauthorized_with_metadata_and_scope() {
        let (meta, scope) = resolve_probe_target(
            reqwest::StatusCode::UNAUTHORIZED,
            Some(r#"Bearer resource_metadata="https://auth.example.com/meta", scope="mcp:read""#),
            "http://example.com/mcp/",
        );
        assert_eq!(meta, "https://auth.example.com/meta");
        assert_eq!(scope.as_deref(), Some("mcp:read"));
    }

    #[test]
    fn test_resolve_probe_target_unauthorized_without_metadata() {
        let (meta, scope) = resolve_probe_target(
            reqwest::StatusCode::UNAUTHORIZED,
            Some("Bearer error=\"invalid_token\""),
            "http://example.com/mcp/",
        );
        assert_eq!(meta, "http://example.com/mcp/.well-known/oauth-protected-resource");
        assert_eq!(scope, None);
    }

    #[test]
    fn test_resolve_probe_target_ok() {
        let (meta, scope) = resolve_probe_target(reqwest::StatusCode::OK, None, "http://example.com/mcp");
        assert_eq!(meta, "http://example.com/mcp/.well-known/oauth-protected-resource");
        assert_eq!(scope, None);
    }

    #[tokio::test]
    async fn test_refresh_mcp_token_non_oauth() {
        let cred = StoredCredential::api_key("plain-api-key");
        let err = refresh_mcp_token("http://127.0.0.1:0/token", None, &cred)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Cannot refresh non-OAuth credential"));
    }

    #[tokio::test]
    async fn test_refresh_mcp_token_missing_refresh() {
        let cred = StoredCredential::oauth("access-token", None, None);
        let err = refresh_mcp_token("http://127.0.0.1:0/token", None, &cred)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("No refresh token available"));
    }

    #[tokio::test]
    async fn test_refresh_mcp_token_http_failure() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let token_endpoint = format!("http://{addr}/token");

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 1024];
            let _ = socket.read(&mut buf).await.unwrap();
            let http_resp = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 5\r\n\r\nerror";
            socket.write_all(http_resp.as_bytes()).await.unwrap();
        });

        let cred = StoredCredential::oauth("tok", Some("ref".to_string()), None);
        let err = refresh_mcp_token(&token_endpoint, None, &cred).await.unwrap_err();
        assert!(err.to_string().contains("Token refresh failed: error"));
    }

    #[tokio::test]
    async fn test_refresh_mcp_token_invalid_json() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let token_endpoint = format!("http://{addr}/token");

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 1024];
            let _ = socket.read(&mut buf).await.unwrap();
            let http_resp = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 7\r\n\r\ninvalid";
            socket.write_all(http_resp.as_bytes()).await.unwrap();
        });

        let cred = StoredCredential::oauth("tok", Some("ref".to_string()), None);
        let err = refresh_mcp_token(&token_endpoint, None, &cred).await.unwrap_err();
        assert!(err.to_string().contains("Failed to parse token response"));
    }

    #[tokio::test]
    async fn test_get_valid_mcp_token() {
        let dir = tempdir().unwrap();
        let mut store = AuthStore::load(dir.path().join("auth.json")).unwrap();

        assert_eq!(get_valid_mcp_token("unknown-server", &mut store).await, None);

        let cred = StoredCredential::api_key("mcp-secret");
        store.set_credential("mcp:known-server", cred).unwrap();

        assert_eq!(
            get_valid_mcp_token("known-server", &mut store).await,
            Some("mcp-secret".to_string())
        );
    }

    #[tokio::test]
    async fn test_login_mcp_server_missing_url() {
        let dir = tempdir().unwrap();
        let mut store = AuthStore::load(dir.path().join("auth.json")).unwrap();
        let config = McpServerConfig {
            url: None,
            ..Default::default()
        };
        let callbacks = MockCallbacks { auto_callback: false };
        let err = login_mcp_server("missing-srv", &config, &mut store, &callbacks)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("has no URL configured for OAuth"));
    }

    #[tokio::test]
    async fn test_login_mcp_server_probe_failure() {
        let dir = tempdir().unwrap();
        let mut store = AuthStore::load(dir.path().join("auth.json")).unwrap();
        let config = McpServerConfig {
            url: Some("http://127.0.0.1:1/nonexistent".to_string()),
            ..Default::default()
        };
        let callbacks = MockCallbacks { auto_callback: false };
        let err = login_mcp_server("bad-srv", &config, &mut store, &callbacks)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Failed to probe MCP server"));
    }

    #[tokio::test]
    async fn test_discover_mcp_auth_metadata_no_servers() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let meta_url = format!("http://{addr}/meta");

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 1024];
            let _ = socket.read(&mut buf).await.unwrap();
            let body = r#"{"authorization_servers":[]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(resp.as_bytes()).await.unwrap();
        });

        let client = crate::auth::http::http_client();
        let err = discover_mcp_auth_metadata(client, &meta_url).await.unwrap_err();
        assert!(
            err.to_string()
                .contains("No authorization servers advertised by MCP server")
        );
    }

    #[tokio::test]
    async fn test_login_mcp_server_full_flow() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_url = format!("http://{addr}/mcp");

        tokio::spawn(async move {
            loop {
                let (mut socket, _) = match listener.accept().await {
                    Ok(s) => s,
                    Err(_) => break,
                };
                let mut buf = vec![0u8; 2048];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    continue;
                }
                let req = String::from_utf8_lossy(&buf[..n]);
                let first_line = req.lines().next().unwrap_or("");

                let (status_line, headers, body) = if first_line.starts_with("GET /mcp") {
                    (
                        "HTTP/1.1 401 Unauthorized",
                        format!(
                            "WWW-Authenticate: Bearer resource_metadata=\"http://{addr}/meta\", scope=\"custom_scope\"\r\n"
                        ),
                        String::new(),
                    )
                } else if first_line.starts_with("GET /meta") {
                    let body = format!("{{\"authorization_servers\":[\"http://{addr}/auth\"]}}");
                    (
                        "HTTP/1.1 200 OK",
                        "Content-Type: application/json\r\n".to_string(),
                        body,
                    )
                } else if first_line.starts_with("GET /auth/.well-known/oauth-authorization-server") {
                    let body = format!(
                        "{{\"issuer\":\"http://{addr}/auth\",\"authorization_endpoint\":\"http://{addr}/auth/authorize\",\"token_endpoint\":\"http://{addr}/auth/token\"}}"
                    );
                    (
                        "HTTP/1.1 200 OK",
                        "Content-Type: application/json\r\n".to_string(),
                        body,
                    )
                } else if first_line.starts_with("POST /auth/token") {
                    let body =
                        r#"{"access_token":"login-mcp-token","refresh_token":"login-refresh-token","expires_in":3600}"#
                            .to_string();
                    (
                        "HTTP/1.1 200 OK",
                        "Content-Type: application/json\r\n".to_string(),
                        body,
                    )
                } else {
                    ("HTTP/1.1 404 Not Found", String::new(), String::new())
                };

                let response = format!(
                    "{status_line}\r\nConnection: close\r\n{headers}Content-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });

        let dir = tempdir().unwrap();
        let mut store = AuthStore::load(dir.path().join("auth.json")).unwrap();
        let config = McpServerConfig {
            url: Some(server_url),
            ..Default::default()
        };
        let callbacks = MockCallbacks { auto_callback: true };
        let cred = login_mcp_server("srv", &config, &mut store, &callbacks).await.unwrap();

        assert_eq!(cred.raw_secret(), "login-mcp-token");
        assert_eq!(
            get_valid_mcp_token("srv", &mut store).await,
            Some("login-mcp-token".to_string())
        );
    }

    #[tokio::test]
    async fn test_refresh_mcp_token() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let token_endpoint = format!("http://{addr}/token");

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 1024];
            let _ = socket.read(&mut buf).await.unwrap();

            let resp_body = r#"{"access_token":"new-mcp-token","refresh_token":"new-refresh-token","expires_in":3600}"#;
            let http_resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                resp_body.len(),
                resp_body
            );
            socket.write_all(http_resp.as_bytes()).await.unwrap();
        });

        let cred = StoredCredential::oauth("old-token", Some("old-refresh".to_string()), None);
        let refreshed = refresh_mcp_token(&token_endpoint, None, &cred).await.unwrap();

        assert_eq!(refreshed.raw_secret(), "new-mcp-token");
        if let StoredCredential::OAuth { refresh_token, .. } = refreshed {
            assert_eq!(refresh_token, Some("new-refresh-token".to_string()));
        } else {
            panic!("Expected OAuth credential");
        }
    }
}
