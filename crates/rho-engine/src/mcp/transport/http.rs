use super::super::types::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
use futures::StreamExt;
use rho_harness_core::config::McpTransportKind;
use rho_harness_core::error::{AppError, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::sync::RwLock;

pub const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(30);
pub const MCP_PROTOCOL_VERSION_HEADER: &str = "2025-11-25";

static SHARED_HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    crate::install_crypto_provider();
    reqwest::Client::builder().no_proxy().build().unwrap_or_default()
});

pub struct HttpTransport {
    url: String,
    kind: McpTransportKind,
    headers: BTreeMap<String, String>,
    timeout: Duration,
    session_id: Arc<RwLock<Option<String>>>,
    next_id: AtomicI64,
}

impl HttpTransport {
    pub fn new(
        url: impl Into<String>,
        kind: McpTransportKind,
        headers: BTreeMap<String, String>,
        timeout: Option<Duration>,
    ) -> Arc<Self> {
        Arc::new(Self {
            url: url.into(),
            kind,
            headers,
            timeout: timeout.unwrap_or(DEFAULT_HTTP_TIMEOUT),
            session_id: Arc::new(RwLock::new(None)),
            next_id: AtomicI64::new(1),
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn kind(&self) -> McpTransportKind {
        self.kind
    }

    fn client(&self) -> &'static reqwest::Client {
        &SHARED_HTTP_CLIENT
    }

    async fn build_request_headers(&self) -> reqwest::header::HeaderMap {
        let mut map = reqwest::header::HeaderMap::new();
        map.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );
        map.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/json, text/event-stream"),
        );
        map.insert(
            reqwest::header::HeaderName::from_static("mcp-protocol-version"),
            reqwest::header::HeaderValue::from_static(MCP_PROTOCOL_VERSION_HEADER),
        );

        for (k, v) in &self.headers {
            if let (Ok(name), Ok(val)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_str(v),
            ) {
                map.insert(name, val);
            }
        }

        if let Some(session_id) = self.session_id.read().await.as_deref()
            && let Ok(val) = reqwest::header::HeaderValue::from_str(session_id)
        {
            map.insert(reqwest::header::HeaderName::from_static("mcp-session-id"), val);
        }

        map
    }

    async fn record_session_id(&self, headers: &reqwest::header::HeaderMap) {
        if let Some(val) = headers.get("mcp-session-id").and_then(|v| v.to_str().ok()) {
            let mut lock = self.session_id.write().await;
            if lock.as_deref() != Some(val) {
                *lock = Some(val.to_string());
            }
        }
    }

    async fn parse_sse_response(&self, res: reqwest::Response, target_id: i64) -> Result<Value> {
        let mut stream = res.bytes_stream();
        let mut buffer = String::new();
        let mut data_lines = Vec::new();

        while let Some(chunk_res) = stream.next().await {
            let chunk = chunk_res.map_err(|e| AppError::Plugin(format!("SSE stream error: {e}")))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim_end_matches('\r').to_string();
                buffer.drain(..=pos);

                if line.is_empty() {
                    if !data_lines.is_empty() {
                        let combined = data_lines.join("\n");
                        data_lines.clear();
                        if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(&combined)
                            && resp.id.as_i64() == Some(target_id)
                        {
                            if let Some(err) = resp.error {
                                return Err(AppError::Plugin(format!(
                                    "MCP error: {} (code {})",
                                    err.message, err.code
                                )));
                            }
                            return Ok(resp.result.unwrap_or(Value::Null));
                        }
                    }
                } else if let Some(stripped) = line.strip_prefix("data:") {
                    data_lines.push(stripped.trim().to_string());
                }
            }
        }

        Err(AppError::Plugin(
            "SSE stream closed without returning response".to_string(),
        ))
    }

    pub async fn request(&self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let req_payload = JsonRpcRequest::new(id, method, params);
        let headers = self.build_request_headers().await;

        let post_fut = async {
            let res = self
                .client()
                .post(&self.url)
                .headers(headers)
                .json(&req_payload)
                .send()
                .await
                .map_err(|e| AppError::Plugin(format!("HTTP request to '{}' failed: {e}", self.url)))?;

            let status = res.status();
            let res_headers = res.headers().clone();
            self.record_session_id(&res_headers).await;

            if status == reqwest::StatusCode::UNAUTHORIZED {
                let www_auth = res_headers
                    .get(reqwest::header::WWW_AUTHENTICATE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                return Err(AppError::Auth(format!("Unauthorized: {www_auth}")));
            }

            if !status.is_success() {
                let body = res.text().await.unwrap_or_default();
                return Err(AppError::Plugin(format!(
                    "HTTP error {status} from '{}': {body}",
                    self.url
                )));
            }

            let content_type = res_headers
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");

            if content_type.contains("text/event-stream") {
                self.parse_sse_response(res, id).await
            } else {
                let json: JsonRpcResponse = res
                    .json()
                    .await
                    .map_err(|e| AppError::Plugin(format!("Failed to parse JSON-RPC response: {e}")))?;

                if let Some(err) = json.error {
                    return Err(AppError::Plugin(format!(
                        "MCP error from {method}: {} (code {})",
                        err.message, err.code
                    )));
                }
                Ok(json.result.unwrap_or(Value::Null))
            }
        };

        match tokio::time::timeout(self.timeout, post_fut).await {
            Ok(res) => res,
            Err(_) => {
                let _ = self
                    .notify("notifications/cancelled", Some(serde_json::json!({ "requestId": id })))
                    .await;
                Err(AppError::Plugin(format!(
                    "MCP request '{method}' to '{}' timed out",
                    self.url
                )))
            }
        }
    }

    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<()> {
        let notif_payload = JsonRpcNotification::new(method, params);
        let headers = self.build_request_headers().await;

        let _ = self
            .client()
            .post(&self.url)
            .headers(headers)
            .json(&notif_payload)
            .send()
            .await;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_http_transport_json_response() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{addr}/mcp");

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 1024];
            let n = socket.read(&mut buf).await.unwrap();
            let req_str = String::from_utf8_lossy(&buf[..n]);
            assert!(req_str.contains("mcp-protocol-version: 2025-11-25"));

            let resp_body = r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#;
            let http_resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                resp_body.len(),
                resp_body
            );
            socket.write_all(http_resp.as_bytes()).await.unwrap();
        });

        let transport = HttpTransport::new(url, McpTransportKind::StreamableHttp, BTreeMap::new(), None);
        let res = transport.request("tools/list", None).await.unwrap();
        assert_eq!(res, serde_json::json!({ "tools": [] }));
    }

    #[tokio::test]
    async fn test_http_transport_sse_streaming() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{addr}/mcp");

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 1024];
            let _ = socket.read(&mut buf).await.unwrap();

            let resp_body =
                "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"status\":\"streaming_ok\"}}\n\n";
            let http_resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{}",
                resp_body
            );
            socket.write_all(http_resp.as_bytes()).await.unwrap();
        });

        let transport = HttpTransport::new(url, McpTransportKind::StreamableHttp, BTreeMap::new(), None);
        let res = transport.request("ping", None).await.unwrap();
        assert_eq!(res, serde_json::json!({ "status": "streaming_ok" }));
    }
}
