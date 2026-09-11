pub mod discovery;
pub mod flow;

use crate::auth::AuthStore;
use discovery::{
    extract_required_scope, extract_resource_metadata_url, fetch_auth_server_metadata,
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
    let res = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to probe MCP server at '{url}': {e}")))?;

    let (metadata_url, scope) = if res.status() == reqwest::StatusCode::UNAUTHORIZED {
        let www_auth = res
            .headers()
            .get(reqwest::header::WWW_AUTHENTICATE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let m_url = extract_resource_metadata_url(www_auth);
        let s = extract_required_scope(www_auth);
        (m_url, s)
    } else {
        (None, None)
    };

    let base_url = url.trim_end_matches('/');
    let meta_target = metadata_url.unwrap_or_else(|| format!("{base_url}/.well-known/oauth-protected-resource"));

    let protected_meta = fetch_protected_resource_metadata(client, &meta_target).await?;
    let auth_server = protected_meta
        .authorization_servers
        .first()
        .ok_or_else(|| AppError::Auth("No authorization servers advertised by MCP server".to_string()))?;

    let auth_meta = fetch_auth_server_metadata(client, auth_server).await?;

    let cred = execute_mcp_pkce_flow(
        server_name,
        &auth_meta.authorization_endpoint,
        &auth_meta.token_endpoint,
        scope.as_deref(),
        None,
        callbacks,
    )
    .await?;

    let key = format!("mcp:{server_name}");
    auth_store.set_credential(&key, cred.clone())?;
    let _ = auth_store.save_async().await;

    Ok(cred)
}

pub async fn get_valid_mcp_token(server_name: &str, auth_store: &mut AuthStore) -> Option<String> {
    let key = format!("mcp:{server_name}");
    auth_store.get_key(&key).await.ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

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
