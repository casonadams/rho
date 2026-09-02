//! Ephemeral loopback HTTP callback listener for OAuth 2.0 PKCE redirection.

use rho_harness_core::error::{AppError, Result};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub struct LoopbackServer {
    listener: TcpListener,
    port: u16,
}

#[derive(Debug, Clone)]
pub struct CallbackParams {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

impl LoopbackServer {
    pub async fn bind() -> Result<Self> {
        Self::bind_port(0).await
    }

    pub async fn bind_port(port: u16) -> Result<Self> {
        let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| AppError::Auth(format!("Failed to bind loopback OAuth listener on port {port}: {e}")))?;
        let bound_port = listener
            .local_addr()
            .map_err(|e| AppError::Auth(format!("Failed to get loopback port: {e}")))?
            .port();
        Ok(Self {
            listener,
            port: bound_port,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn redirect_uri(&self, path: &str) -> String {
        let path = if path.starts_with('/') { path } else { "/callback" };
        format!("http://localhost:{}{path}", self.port)
    }

    pub async fn wait_for_callback(self, timeout: Duration) -> Result<CallbackParams> {
        tokio::select! {
            result = self.handle_connection() => result,
            _ = tokio::time::sleep(timeout) => Err(AppError::Auth("OAuth authorization timed out (2 minutes)".to_string())),
        }
    }

    async fn handle_connection(&self) -> Result<CallbackParams> {
        let (mut socket, _) = self
            .listener
            .accept()
            .await
            .map_err(|e| AppError::Auth(format!("Loopback connection failed: {e}")))?;

        let mut buffer = [0u8; 4096];
        let bytes_read = socket
            .read(&mut buffer)
            .await
            .map_err(|e| AppError::Auth(format!("Failed to read callback request: {e}")))?;
        let request = String::from_utf8_lossy(&buffer[..bytes_read]);

        let params = parse_http_get_params(&request);

        let (status, body) = if params.code.is_some() {
            (
                "200 OK",
                "<!DOCTYPE html><html><head><title>Authentication Successful</title></head>\
                 <body style='font-family:system-ui,-apple-system,sans-serif;text-align:center;padding:60px 20px;background:#121212;color:#eee;'>\
                 <div style='max-width:480px;margin:0 auto;background:#1e1e1e;border-radius:12px;padding:32px;border:1px solid #333;'>\
                 <h2 style='color:#10a37f;margin-top:0;'>Authentication Successful!</h2>\
                 <p style='color:#aaa;line-height:1.6;'>You can close this tab and return to <strong>rho</strong> in your terminal.</p>\
                 </div></body></html>",
            )
        } else {
            (
                "400 Bad Request",
                "<!DOCTYPE html><html><head><title>Authentication Failed</title></head>\
                 <body style='font-family:system-ui,-apple-system,sans-serif;text-align:center;padding:60px 20px;background:#121212;color:#eee;'>\
                 <div style='max-width:480px;margin:0 auto;background:#1e1e1e;border-radius:12px;padding:32px;border:1px solid #333;'>\
                 <h2 style='color:#ef4444;margin-top:0;'>Authentication Failed</h2>\
                 <p style='color:#aaa;'>Please check your terminal for details.</p>\
                 </div></body></html>",
            )
        };

        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = socket.write_all(response.as_bytes()).await;
        let _ = socket.flush().await;

        Ok(params)
    }
}

pub fn parse_http_get_params(request: &str) -> CallbackParams {
    let first_line = request.lines().next().unwrap_or_default();
    let mut parts = first_line.split_whitespace();
    let _method = parts.next();
    let path = parts.next().unwrap_or("/");

    let query = path.split_once('?').map(|(_, q)| q).unwrap_or_default();
    let mut map = HashMap::new();
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            map.insert(k.to_string(), percent_decode_str(v));
        }
    }

    CallbackParams {
        code: map.remove("code"),
        state: map.remove("state"),
        error: map.remove("error"),
        error_description: map.remove("error_description"),
    }
}

fn percent_decode_str(s: &str) -> String {
    percent_encoding::percent_decode_str(s).decode_utf8_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_callback_url_params_correctly() {
        let req = "GET /auth/callback?code=abc-123&state=xyz-789 HTTP/1.1\r\nHost: localhost:1455\r\n\r\n";
        let params = parse_http_get_params(req);
        assert_eq!(params.code.as_deref(), Some("abc-123"));
        assert_eq!(params.state.as_deref(), Some("xyz-789"));
        assert!(params.error.is_none());
    }

    #[test]
    fn parses_oauth_errors_correctly() {
        let req = "GET /auth/callback?error=access_denied&error_description=User%20denied HTTP/1.1\r\n";
        let params = parse_http_get_params(req);
        assert_eq!(params.error.as_deref(), Some("access_denied"));
        assert_eq!(params.error_description.as_deref(), Some("User denied"));
    }
}
