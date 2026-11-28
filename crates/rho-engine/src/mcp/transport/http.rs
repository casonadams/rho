use super::super::oauth::{McpOAuth, login_hint};
use super::super::types::{JsonRpcNotification, JsonRpcRequest};
use super::Inbound;
use super::route::{Routed, reply_to_server_request, route_message, server_error};
use crate::http::CLIENT;
use crate::provider::sse::SseLineDecoder;
use futures::StreamExt;
use rho_harness_core::config::McpTransportKind;
use rho_harness_core::error::{AppError, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::sync::RwLock;

pub const MCP_PROTOCOL_VERSION_HEADER: &str = "2025-11-25";

pub struct HttpTransport {
    url: String,
    kind: McpTransportKind,
    headers: BTreeMap<String, String>,
    session_id: Arc<RwLock<Option<String>>>,
    next_id: AtomicI64,
    oauth: Option<Arc<McpOAuth>>,
    inbound: Inbound,
}

enum Reply {
    Value(Value),
    Unauthorized(String),
}

fn complete_sse_message(line: &str, data_lines: &mut Vec<String>) -> Option<Value> {
    if !line.is_empty() {
        if let Some(stripped) = line.strip_prefix("data:") {
            data_lines.push(stripped.trim().to_string());
        }
        return None;
    }
    if data_lines.is_empty() {
        return None;
    }
    let combined = data_lines.join("\n");
    data_lines.clear();
    serde_json::from_str(&combined).ok()
}

fn scan_sse_chunk(decoder: &mut SseLineDecoder, data_lines: &mut Vec<String>, chunk: &[u8]) -> Vec<Value> {
    decoder
        .decode_raw_lines(chunk)
        .iter()
        .filter_map(|line| complete_sse_message(line, data_lines))
        .collect()
}

fn unauthorized_challenge(res: &reqwest::Response) -> Option<String> {
    (res.status() == reqwest::StatusCode::UNAUTHORIZED).then(|| {
        res.headers()
            .get(reqwest::header::WWW_AUTHENTICATE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string()
    })
}

fn insert_configured_headers(
    map: &mut reqwest::header::HeaderMap,
    headers: &BTreeMap<String, String>,
    url: &str,
) -> Result<()> {
    for (k, v) in headers {
        let name = reqwest::header::HeaderName::from_bytes(k.as_bytes());
        let value = reqwest::header::HeaderValue::from_str(v);
        let (Ok(name), Ok(value)) = (name, value) else {
            return Err(AppError::Config(format!(
                "MCP header '{k}' configured for '{url}' is not a valid HTTP header"
            )));
        };
        map.insert(name, value);
    }
    Ok(())
}

impl HttpTransport {
    pub fn new(
        url: impl Into<String>,
        kind: McpTransportKind,
        headers: BTreeMap<String, String>,
        oauth: Option<Arc<McpOAuth>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            url: url.into(),
            kind,
            headers,
            session_id: Arc::new(RwLock::new(None)),
            next_id: AtomicI64::new(1),
            oauth,
            inbound: Inbound::default(),
        })
    }

    pub(crate) fn inbound(&self) -> &Inbound {
        &self.inbound
    }

    pub(crate) fn next_id(&self) -> i64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn kind(&self) -> McpTransportKind {
        self.kind
    }

    async fn build_request_headers(&self, bearer: Option<&str>) -> Result<reqwest::header::HeaderMap> {
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

        insert_configured_headers(&mut map, &self.headers, &self.url)?;

        if let Some(token) = bearer {
            let val = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| {
                AppError::Auth(format!(
                    "access token for '{}' is not a valid Authorization header",
                    self.url
                ))
            })?;
            map.insert(reqwest::header::AUTHORIZATION, val);
        }

        if let Some(session_id) = self.session_id.read().await.as_deref() {
            let val = reqwest::header::HeaderValue::from_str(session_id).map_err(|_| {
                AppError::Mcp(format!(
                    "session id from '{}' is not a valid mcp-session-id header",
                    self.url
                ))
            })?;
            map.insert(reqwest::header::HeaderName::from_static("mcp-session-id"), val);
        }

        Ok(map)
    }

    async fn record_session_id(&self, headers: &reqwest::header::HeaderMap) {
        if let Some(val) = headers.get("mcp-session-id").and_then(|v| v.to_str().ok()) {
            let mut lock = self.session_id.write().await;
            if lock.as_deref() != Some(val) {
                *lock = Some(val.to_string());
            }
        }
    }

    async fn parse_sse_response(&self, res: reqwest::Response, method: &str, target_id: i64) -> Result<Value> {
        let mut stream = res.bytes_stream();
        let mut decoder = SseLineDecoder::new();
        let mut data_lines = Vec::new();

        while let Some(chunk_res) = stream.next().await {
            let chunk = chunk_res.map_err(|e| AppError::Mcp(format!("SSE stream error: {e}")))?;
            for message in scan_sse_chunk(&mut decoder, &mut data_lines, &chunk) {
                if let Some(value) = self.handle_sse_message(message, method, target_id).await? {
                    return Ok(value);
                }
            }
        }

        Err(AppError::Mcp(
            "SSE stream closed without returning response".to_string(),
        ))
    }

    async fn handle_sse_message(&self, message: Value, method: &str, target_id: i64) -> Result<Option<Value>> {
        match route_message(message) {
            Some(Routed::Notification(notification)) => self.inbound.dispatch(&notification),
            Some(Routed::Request { id, method: request }) => {
                self.post_message(&reply_to_server_request(id, &request)).await?;
            }
            Some(Routed::Response(id, outcome)) if id.as_i64() == Some(target_id) => {
                return outcome.map(Some).map_err(|e| server_error(method, &e));
            }
            Some(Routed::Response(..)) | None => {}
        }
        Ok(None)
    }

    async fn bearer(&self) -> Result<Option<String>> {
        match &self.oauth {
            Some(oauth) => oauth.bearer().await,
            None => Ok(None),
        }
    }

    fn unauthorized(&self, www_auth: &str) -> AppError {
        match &self.oauth {
            Some(oauth) => AppError::Auth(format!(
                "MCP server '{}' requires authorization. {}",
                oauth.server(),
                login_hint(oauth.server())
            )),
            None => AppError::Auth(format!("Unauthorized: {www_auth}")),
        }
    }

    async fn post(&self, body: &impl serde::Serialize, bearer: Option<&str>) -> Result<reqwest::Response> {
        let res = CLIENT
            .post(&self.url)
            .headers(self.build_request_headers(bearer).await?)
            .json(body)
            .send()
            .await
            .map_err(|e| {
                let message = format!("HTTP request to '{}' failed: {e}", self.url);
                if e.is_connect() {
                    AppError::McpNotDelivered(message)
                } else {
                    AppError::McpConnectionLost(message)
                }
            })?;

        let had_session = self.session_id.read().await.is_some();
        self.record_session_id(res.headers()).await;

        if res.status() == reqwest::StatusCode::NOT_FOUND && had_session {
            return Err(AppError::McpNotDelivered(format!(
                "MCP session at '{}' expired",
                self.url
            )));
        }
        Ok(res)
    }

    async fn http_error(&self, res: reqwest::Response) -> AppError {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        AppError::Mcp(format!("HTTP error {status} from '{}': {body}", self.url))
    }

    async fn post_message(&self, body: &impl serde::Serialize) -> Result<()> {
        let bearer = self.bearer().await?;
        let res = self.post(body, bearer.as_deref()).await?;
        if let Some(www_auth) = unauthorized_challenge(&res) {
            return Err(self.unauthorized(&www_auth));
        }
        if !res.status().is_success() {
            return Err(self.http_error(res).await);
        }
        Ok(())
    }

    async fn send(&self, method: &str, id: i64, payload: &JsonRpcRequest, bearer: Option<&str>) -> Result<Reply> {
        let res = self.post(payload, bearer).await?;
        if let Some(www_auth) = unauthorized_challenge(&res) {
            return Ok(Reply::Unauthorized(www_auth));
        }
        if !res.status().is_success() {
            return Err(self.http_error(res).await);
        }

        let is_sse = res
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|content_type| content_type.contains("text/event-stream"));
        if is_sse {
            return self.parse_sse_response(res, method, id).await.map(Reply::Value);
        }
        let message: Value = res
            .json()
            .await
            .map_err(|e| AppError::Mcp(format!("Failed to parse JSON-RPC response: {e}")))?;
        match route_message(message) {
            Some(Routed::Response(_, outcome)) => outcome.map(Reply::Value).map_err(|e| server_error(method, &e)),
            _ => Err(AppError::Mcp(
                "Failed to parse JSON-RPC response: not a response object".to_string(),
            )),
        }
    }

    async fn send_authorized(&self, method: &str, id: i64, payload: &JsonRpcRequest) -> Result<Value> {
        let token = self.bearer().await?;
        let www_auth = match self.send(method, id, payload, token.as_deref()).await? {
            Reply::Value(value) => return Ok(value),
            Reply::Unauthorized(www_auth) => www_auth,
        };
        let retry = match (&self.oauth, token) {
            (Some(oauth), Some(rejected)) => oauth.refresh_after_rejection(&rejected).await?,
            _ => None,
        };
        let Some(fresh) = retry else {
            return Err(self.unauthorized(&www_auth));
        };
        match self.send(method, id, payload, Some(&fresh)).await? {
            Reply::Value(value) => Ok(value),
            Reply::Unauthorized(www_auth) => Err(self.unauthorized(&www_auth)),
        }
    }

    pub(crate) async fn send_request(&self, id: i64, method: &str, params: Option<Value>) -> Result<Value> {
        let payload = JsonRpcRequest::new(id, method, params);
        self.send_authorized(method, id, &payload).await
    }

    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<()> {
        self.post_message(&JsonRpcNotification::new(method, params)).await
    }
}

#[cfg(test)]
mod tests {
    use super::super::McpTransport;
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

        let transport = McpTransport::new_http(url, McpTransportKind::StreamableHttp, BTreeMap::new(), None, None);
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

        let transport = McpTransport::new_http(url, McpTransportKind::StreamableHttp, BTreeMap::new(), None, None);
        let res = transport.request("ping", None).await.unwrap();
        assert_eq!(res, serde_json::json!({ "status": "streaming_ok" }));
    }

    #[tokio::test]
    async fn test_http_transport_accessors() {
        let transport = HttpTransport::new(
            "http://example.com/mcp",
            McpTransportKind::StreamableHttp,
            BTreeMap::new(),
            None,
        );
        assert_eq!(transport.kind(), McpTransportKind::StreamableHttp);
        assert_eq!(transport.url(), "http://example.com/mcp");
    }

    #[tokio::test]
    async fn test_http_transport_headers_and_session() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{addr}/mcp");

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 1024];
            let n = socket.read(&mut buf).await.unwrap();
            let req_str = String::from_utf8_lossy(&buf[..n]);
            assert!(req_str.contains("x-custom-key: custom-val"));

            let resp_body = r#"{"jsonrpc":"2.0","id":1,"result":true}"#;
            let http_resp = format!(
                "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nMcp-Session-Id: session-xyz\r\nContent-Length: {}\r\n\r\n{}",
                resp_body.len(),
                resp_body
            );
            socket.write_all(http_resp.as_bytes()).await.unwrap();

            let (mut socket2, _) = listener.accept().await.unwrap();
            let n2 = socket2.read(&mut buf).await.unwrap();
            let req_str2 = String::from_utf8_lossy(&buf[..n2]);
            assert!(req_str2.contains("mcp-session-id: session-xyz"));
            socket2.write_all(http_resp.as_bytes()).await.unwrap();
        });

        let mut headers = BTreeMap::new();
        headers.insert("X-Custom-Key".to_string(), "custom-val".to_string());
        let transport = McpTransport::new_http(url, McpTransportKind::StreamableHttp, headers, None, None);
        let res1 = transport.request("init", None).await.unwrap();
        assert_eq!(res1, serde_json::json!(true));
        let res2 = transport.request("next", None).await.unwrap();
        assert_eq!(res2, serde_json::json!(true));
    }

    #[tokio::test]
    async fn test_http_transport_error_status_and_auth() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{addr}/mcp");

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 1024];
            let _ = socket.read(&mut buf).await.unwrap();
            let http_resp = "HTTP/1.1 401 Unauthorized\r\nConnection: close\r\nWWW-Authenticate: Bearer error=\"invalid_token\"\r\nContent-Length: 0\r\n\r\n";
            socket.write_all(http_resp.as_bytes()).await.unwrap();

            let (mut socket2, _) = listener.accept().await.unwrap();
            let _ = socket2.read(&mut buf).await.unwrap();
            let err_body = "server crashed";
            let http_resp2 = format!(
                "HTTP/1.1 500 Internal Server Error\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                err_body.len(),
                err_body
            );
            socket2.write_all(http_resp2.as_bytes()).await.unwrap();

            let (mut socket3, _) = listener.accept().await.unwrap();
            let _ = socket3.read(&mut buf).await.unwrap();
            let rpc_err = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32600,"message":"Bad request"}}"#;
            let http_resp3 = format!(
                "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                rpc_err.len(),
                rpc_err
            );
            socket3.write_all(http_resp3.as_bytes()).await.unwrap();
        });

        let transport = McpTransport::new_http(url, McpTransportKind::StreamableHttp, BTreeMap::new(), None, None);
        let auth_err = transport.request("ping", None).await.unwrap_err();
        assert!(auth_err.to_string().contains("Unauthorized"));

        let server_err = transport.request("ping", None).await.unwrap_err();
        assert!(server_err.to_string().contains("HTTP error 500"));

        let rpc_err = transport.request("ping", None).await.unwrap_err();
        assert!(rpc_err.to_string().contains("MCP error from ping"));
    }

    async fn loopback() -> (TcpListener, String) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        (listener, url)
    }

    async fn answer_once(listener: &TcpListener, response: &str) -> Value {
        let (mut socket, _) = listener.accept().await.unwrap();
        let request = crate::mcp::test_server::read_request(&mut socket).await.unwrap();
        socket.write_all(response.as_bytes()).await.unwrap();
        request
    }

    fn transport(url: String, headers: BTreeMap<String, String>) -> Arc<McpTransport> {
        let idle = Some(std::time::Duration::from_secs(1));
        McpTransport::new_http(url, McpTransportKind::StreamableHttp, headers, idle, None)
    }

    const ACCEPTED: &str = "HTTP/1.1 202 Accepted\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";
    const SSE_HEAD: &str = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

    #[tokio::test]
    async fn server_ping_inside_an_sse_stream_is_answered() {
        let (listener, url) = loopback().await;
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            crate::mcp::test_server::read_request(&mut stream).await;
            let ping = r#"{"jsonrpc":"2.0","id":"srv-1","method":"ping"}"#;
            stream
                .write_all(format!("{SSE_HEAD}data: {ping}\n\n").as_bytes())
                .await
                .unwrap();
            let reply = answer_once(&listener, ACCEPTED).await;
            let done = serde_json::json!({ "jsonrpc": "2.0", "id": 1, "result": reply });
            stream.write_all(format!("data: {done}\n\n").as_bytes()).await.unwrap();
        });

        let res = transport(url, BTreeMap::new())
            .request("tools/list", None)
            .await
            .unwrap();

        assert_eq!(
            res,
            serde_json::json!({ "jsonrpc": "2.0", "id": "srv-1", "result": {} })
        );
    }

    #[tokio::test]
    async fn sse_error_responses_name_the_method() {
        let (listener, url) = loopback().await;
        tokio::spawn(async move {
            let err = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"Bad params"}}"#;
            answer_once(&listener, &format!("{SSE_HEAD}data: {err}\n\n")).await;
        });

        let err = transport(url, BTreeMap::new())
            .request("tools/call", None)
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::Mcp(_)), "{err}");
        assert!(
            err.to_string()
                .contains("MCP error from tools/call: Bad params (code -32602)"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn invalid_configured_headers_are_reported_by_name_without_the_value() {
        for (name, value) in [("X-Bad", "secret\nvalue"), ("Bad Name", "secret")] {
            let (listener, url) = loopback().await;
            drop(listener);
            let headers = BTreeMap::from([(name.to_string(), value.to_string())]);

            let err = transport(url, headers).request("tools/list", None).await.unwrap_err();

            assert!(matches!(err, AppError::Config(_)), "{err}");
            assert!(err.to_string().contains(&format!("'{name}'")), "{err}");
            assert!(!err.to_string().contains("secret"), "{err}");
        }
    }

    #[tokio::test]
    async fn notify_reports_rejected_notifications() {
        let cases = [
            ("500 Internal Server Error", "HTTP error 500"),
            ("401 Unauthorized", "Unauthorized"),
        ];
        for (status, expected) in cases {
            let (listener, url) = loopback().await;
            let response = format!("HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n");
            tokio::spawn(async move { answer_once(&listener, &response).await });

            let err = transport(url, BTreeMap::new())
                .notify("notifications/initialized", None)
                .await
                .unwrap_err();

            assert!(err.to_string().contains(expected), "{status}: {err}");
        }
    }

    #[tokio::test]
    async fn notify_reports_a_refused_connection_as_not_delivered() {
        let (listener, url) = loopback().await;
        drop(listener);

        let err = transport(url, BTreeMap::new())
            .notify("notifications/initialized", None)
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::McpNotDelivered(_)), "{err}");
    }

    #[tokio::test]
    async fn notify_records_the_session_id() {
        let (listener, url) = loopback().await;
        let server = tokio::spawn(async move {
            let accepted =
                "HTTP/1.1 202 Accepted\r\nConnection: close\r\nMcp-Session-Id: s-42\r\nContent-Length: 0\r\n\r\n";
            answer_once(&listener, accepted).await;
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = socket.read(&mut buf).await.unwrap();
            String::from_utf8_lossy(&buf[..n]).to_string()
        });
        let transport = transport(url, BTreeMap::new());

        transport.notify("notifications/initialized", None).await.unwrap();
        let _ = transport.request("tools/list", None).await;

        assert!(server.await.unwrap().contains("mcp-session-id: s-42"));
    }

    #[test]
    fn sse_chunk_split_inside_multibyte_character_decodes_intact() {
        let event = "data: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":\"héllo\"}\n\n".as_bytes();
        let split = event.iter().position(|b| *b == 0xC3).unwrap() + 1;
        let mut decoder = SseLineDecoder::new();
        let mut data_lines = Vec::new();
        assert!(scan_sse_chunk(&mut decoder, &mut data_lines, &event[..split]).is_empty());
        let messages = scan_sse_chunk(&mut decoder, &mut data_lines, &event[split..]);
        assert_eq!(
            messages,
            [serde_json::json!({ "jsonrpc": "2.0", "id": 7, "result": "héllo" })]
        );
    }
}
