use crate::lsp::LspDiagnostic;
use rho_harness_core::error::AppError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

fn detect_language_id(ext: &str) -> &'static str {
    const MAPPINGS: &[(&str, &str)] = &[
        ("rs", "rust"),
        ("ts", "typescript"),
        ("js", "javascript"),
        ("go", "go"),
        ("py", "python"),
    ];
    for &(e, lang) in MAPPINGS {
        if e == ext {
            return lang;
        }
    }
    "plaintext"
}

pub fn encode_frame(payload: &[u8]) -> Vec<u8> {
    let header = format!("Content-Length: {}\r\n\r\n", payload.len());
    let mut buf = Vec::with_capacity(header.len() + payload.len());
    buf.extend_from_slice(header.as_bytes());
    buf.extend_from_slice(payload);
    buf
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_content_length(headers: &[u8]) -> Result<usize, AppError> {
    let s = std::str::from_utf8(headers).map_err(|e| AppError::Tool(format!("invalid header encoding: {e}")))?;
    for line in s.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            return rest
                .trim()
                .parse::<usize>()
                .map_err(|e| AppError::Tool(format!("invalid Content-Length: {e}")));
        }
        if let Some(rest) = trimmed.strip_prefix("content-length:") {
            return rest
                .trim()
                .parse::<usize>()
                .map_err(|e| AppError::Tool(format!("invalid Content-Length: {e}")));
        }
    }
    Err(AppError::Tool("missing Content-Length header in LSP frame".to_string()))
}

pub fn extract_next_frame(buffer: &mut Vec<u8>) -> Result<Option<Vec<u8>>, AppError> {
    let header_end = match find_header_end(buffer) {
        Some(pos) => pos,
        None => return Ok(None),
    };
    let content_len = parse_content_length(&buffer[..header_end])?;
    let body_start = header_end + 4;
    let total_len = body_start + content_len;
    if buffer.len() < total_len {
        return Ok(None);
    }
    let body = buffer[body_start..total_len].to_vec();
    buffer.drain(..total_len);
    Ok(Some(body))
}

pub async fn read_frame<R: AsyncRead + Unpin>(
    reader: &mut R,
    buffer: &mut Vec<u8>,
) -> Result<Option<Vec<u8>>, AppError> {
    loop {
        if let Some(frame) = extract_next_frame(buffer)? {
            return Ok(Some(frame));
        }
        let mut chunk = [0u8; 4096];
        let n = reader
            .read(&mut chunk)
            .await
            .map_err(|e| AppError::Tool(format!("LSP read failed: {e}")))?;
        if n == 0 {
            if buffer.is_empty() {
                return Ok(None);
            }
            return Err(AppError::Tool("unexpected EOF while reading LSP frame".to_string()));
        }
        buffer.extend_from_slice(&chunk[..n]);
    }
}

#[derive(Serialize)]
struct JsonRpcRequest<T> {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    params: T,
}

#[derive(Serialize)]
struct JsonRpcNotification<T> {
    jsonrpc: &'static str,
    method: String,
    params: T,
}

#[derive(Deserialize, Debug)]
struct JsonRpcResponse {
    id: Option<serde_json::Value>,
    method: Option<String>,
    params: Option<serde_json::Value>,
    result: Option<serde_json::Value>,
    error: Option<RawJsonRpcError>,
}

#[derive(Deserialize, Debug)]
pub struct RawJsonRpcError {
    pub code: i64,
    pub message: String,
}

#[derive(Deserialize)]
struct PublishDiagnosticsParams {
    uri: String,
    diagnostics: Vec<LspDiagnostic>,
}

type PendingMap =
    Arc<tokio::sync::Mutex<HashMap<u64, tokio::sync::oneshot::Sender<Result<serde_json::Value, String>>>>>;
type DiagnosticsMap = Arc<tokio::sync::RwLock<HashMap<String, Vec<LspDiagnostic>>>>;

pub struct LspClient {
    writer: Arc<tokio::sync::Mutex<Box<dyn AsyncWrite + Send + Unpin>>>,
    pending: PendingMap,
    diagnostics: DiagnosticsMap,
    next_id: AtomicU64,
    base_dir: PathBuf,
    connected: Arc<AtomicBool>,
}

impl LspClient {
    pub fn new<R, W>(reader: R, writer: W, base_dir: PathBuf) -> Self
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let pending = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let diagnostics = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
        let connected = Arc::new(AtomicBool::new(true));

        let pending_clone = Arc::clone(&pending);
        let diag_clone = Arc::clone(&diagnostics);
        let connected_clone = Arc::clone(&connected);

        tokio::spawn(async move {
            Self::reader_loop(reader, pending_clone, diag_clone, connected_clone).await;
        });

        Self {
            writer: Arc::new(tokio::sync::Mutex::new(Box::new(writer))),
            pending,
            diagnostics,
            next_id: AtomicU64::new(1),
            base_dir,
            connected,
        }
    }

    async fn reader_loop<R: AsyncRead + Unpin>(
        mut reader: R,
        pending: PendingMap,
        diagnostics: DiagnosticsMap,
        connected: Arc<AtomicBool>,
    ) {
        let mut buf = Vec::new();
        while let Ok(Some(frame)) = read_frame(&mut reader, &mut buf).await {
            Self::dispatch_frame(&frame, &pending, &diagnostics).await;
        }
        connected.store(false, Ordering::SeqCst);
        let mut lock = pending.lock().await;
        for (_id, tx) in lock.drain() {
            let _ = tx.send(Err("LSP process connection closed".to_string()));
        }
    }

    async fn dispatch_frame(frame: &[u8], pending: &PendingMap, diagnostics: &DiagnosticsMap) {
        let msg: JsonRpcResponse = match serde_json::from_slice(frame) {
            Ok(m) => m,
            Err(_) => return,
        };
        if let Some(id_val) = msg.id {
            if let Some(id) = id_val.as_u64() {
                let mut lock = pending.lock().await;
                if let Some(tx) = lock.remove(&id) {
                    if let Some(err) = msg.error {
                        let _ = tx.send(Err(err.message));
                    } else {
                        let _ = tx.send(Ok(msg.result.unwrap_or(serde_json::Value::Null)));
                    }
                }
            }
        } else if let Some(method) = msg.method
            && method == "textDocument/publishDiagnostics"
            && let Some(params) = msg.params
            && let Ok(parsed) = serde_json::from_value::<PublishDiagnosticsParams>(params)
        {
            let mut lock = diagnostics.write().await;
            lock.insert(parsed.uri, parsed.diagnostics);
        }
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    pub async fn send_request(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, AppError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let mut lock = self.pending.lock().await;
            lock.insert(id, tx);
        }
        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        };
        let body = serde_json::to_vec(&req).map_err(|e| AppError::Tool(format!("failed to serialize request: {e}")))?;
        let frame = encode_frame(&body);
        {
            let mut writer = self.writer.lock().await;
            writer
                .write_all(&frame)
                .await
                .map_err(|e| AppError::Tool(format!("failed to write to LSP stdin: {e}")))?;
            writer
                .flush()
                .await
                .map_err(|e| AppError::Tool(format!("failed to flush LSP stdin: {e}")))?;
        }
        match tokio::time::timeout(std::time::Duration::from_secs(10), rx).await {
            Ok(Ok(Ok(val))) => Ok(val),
            Ok(Ok(Err(err))) => Err(AppError::Tool(format!("LSP server error: {err}"))),
            Ok(Err(_)) => Err(AppError::Tool("LSP server response channel dropped".to_string())),
            Err(_) => {
                let mut lock = self.pending.lock().await;
                lock.remove(&id);
                Err(AppError::Tool(format!("LSP request '{method}' timed out after 10s")))
            }
        }
    }

    pub async fn send_notification(&self, method: &str, params: serde_json::Value) -> Result<(), AppError> {
        let notif = JsonRpcNotification {
            jsonrpc: "2.0",
            method: method.to_string(),
            params,
        };
        let body =
            serde_json::to_vec(&notif).map_err(|e| AppError::Tool(format!("failed to serialize notification: {e}")))?;
        let frame = encode_frame(&body);
        let mut writer = self.writer.lock().await;
        writer
            .write_all(&frame)
            .await
            .map_err(|e| AppError::Tool(format!("failed to write to LSP stdin: {e}")))?;
        writer
            .flush()
            .await
            .map_err(|e| AppError::Tool(format!("failed to flush LSP stdin: {e}")))?;
        Ok(())
    }

    pub async fn initialize(&self) -> Result<(), AppError> {
        let root_uri = url::Url::from_directory_path(&self.base_dir)
            .map(|u| u.to_string())
            .unwrap_or_else(|_| format!("file://{}", self.base_dir.display()));
        let init_params = serde_json::json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "definition": { "dynamicRegistration": false },
                    "references": { "dynamicRegistration": false },
                    "documentSymbol": { "dynamicRegistration": false },
                    "rename": { "dynamicRegistration": false },
                    "publishDiagnostics": { "relatedInformation": true }
                },
                "workspace": {
                    "applyEdit": true
                }
            }
        });
        self.send_request("initialize", init_params).await?;
        self.send_notification("initialized", serde_json::json!({})).await?;
        Ok(())
    }

    pub async fn notify_did_open(&self, path: &Path, content: &str) -> Result<(), AppError> {
        let uri = url::Url::from_file_path(path)
            .map(|u| u.to_string())
            .unwrap_or_else(|_| format!("file://{}", path.display()));
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let lang_id = detect_language_id(ext);
        let params = serde_json::json!({
            "textDocument": {
                "uri": uri,
                "languageId": lang_id,
                "version": 1,
                "text": content,
            }
        });
        self.send_notification("textDocument/didOpen", params).await
    }

    pub async fn get_diagnostics(&self, target_path: Option<&Path>) -> Vec<(String, Vec<LspDiagnostic>)> {
        let lock = self.diagnostics.read().await;
        let target_uri = target_path.and_then(|p| url::Url::from_file_path(p).ok().map(|u| u.to_string()));
        let mut results = Vec::new();
        for (uri, diags) in lock.iter() {
            if let Some(target) = &target_uri {
                if uri == target {
                    results.push((uri.clone(), diags.clone()));
                }
            } else {
                results.push((uri.clone(), diags.clone()));
            }
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_encoding_and_extraction() {
        let payload = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":null}";
        let encoded = encode_frame(payload);
        let mut buf = encoded.clone();
        let extracted = extract_next_frame(&mut buf).unwrap().expect("frame found");
        assert_eq!(extracted, payload);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_frame_partial_read() {
        let payload = b"{\"jsonrpc\":\"2.0\"}";
        let encoded = encode_frame(payload);
        let mut buf = encoded[..encoded.len() - 3].to_vec();
        assert!(extract_next_frame(&mut buf).unwrap().is_none());
        buf.extend_from_slice(&encoded[encoded.len() - 3..]);
        let extracted = extract_next_frame(&mut buf).unwrap().expect("frame complete");
        assert_eq!(extracted, payload);
    }

    #[test]
    fn test_frame_multiple_in_buffer() {
        let p1 = b"{\"id\":1}";
        let p2 = b"{\"id\":2}";
        let mut buf = encode_frame(p1);
        buf.extend_from_slice(&encode_frame(p2));

        let e1 = extract_next_frame(&mut buf).unwrap().expect("frame 1");
        assert_eq!(e1, p1);
        let e2 = extract_next_frame(&mut buf).unwrap().expect("frame 2");
        assert_eq!(e2, p2);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_frame_missing_content_length() {
        let mut buf = b"Content-Type: application/json\r\n\r\n{}".to_vec();
        assert!(extract_next_frame(&mut buf).is_err());
    }

    #[tokio::test]
    async fn test_mock_client_initialize_and_request() {
        let (client_read, mut server_write) = tokio::io::duplex(4096);
        let (mut server_read, client_write) = tokio::io::duplex(4096);

        let temp_dir = std::env::temp_dir();
        let client = LspClient::new(client_read, client_write, temp_dir);

        tokio::spawn(async move {
            let mut buf = Vec::new();
            let frame = read_frame(&mut server_read, &mut buf).await.unwrap().unwrap();
            let req: serde_json::Value = serde_json::from_slice(&frame).unwrap();
            assert_eq!(req["method"], "initialize");
            let resp = serde_json::json!({
                "jsonrpc": "2.0",
                "id": req["id"],
                "result": { "capabilities": {} }
            });
            let resp_frame = encode_frame(&serde_json::to_vec(&resp).unwrap());
            server_write.write_all(&resp_frame).await.unwrap();

            let notif = read_frame(&mut server_read, &mut buf).await.unwrap().unwrap();
            let notif_val: serde_json::Value = serde_json::from_slice(&notif).unwrap();
            assert_eq!(notif_val["method"], "initialized");

            let def_req = read_frame(&mut server_read, &mut buf).await.unwrap().unwrap();
            let def_val: serde_json::Value = serde_json::from_slice(&def_req).unwrap();
            assert_eq!(def_val["method"], "textDocument/definition");
            let def_resp = serde_json::json!({
                "jsonrpc": "2.0",
                "id": def_val["id"],
                "result": [{
                    "uri": "file:///workspace/src/lib.rs",
                    "range": {
                        "start": { "line": 10, "character": 4 },
                        "end": { "line": 10, "character": 12 }
                    }
                }]
            });
            let def_frame = encode_frame(&serde_json::to_vec(&def_resp).unwrap());
            server_write.write_all(&def_frame).await.unwrap();
        });

        client.initialize().await.expect("initialize succeeds");
        let def_res = client
            .send_request(
                "textDocument/definition",
                serde_json::json!({
                    "textDocument": { "uri": "file:///workspace/src/main.rs" },
                    "position": { "line": 5, "character": 2 }
                }),
            )
            .await
            .expect("request succeeds");

        assert!(def_res.is_array());
        assert_eq!(def_res[0]["range"]["start"]["line"], 10);
    }

    #[tokio::test]
    async fn test_mock_client_publish_diagnostics() {
        let (client_read, mut server_write) = tokio::io::duplex(4096);
        let (_server_read, client_write) = tokio::io::duplex(4096);

        let temp_dir = std::env::temp_dir();
        let client = LspClient::new(client_read, client_write, temp_dir.clone());

        let target_file = temp_dir.join("test.rs");
        let target_uri = url::Url::from_file_path(&target_file).unwrap().to_string();

        let diag_notif = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": target_uri,
                "diagnostics": [{
                    "range": {
                        "start": { "line": 1, "character": 0 },
                        "end": { "line": 1, "character": 10 }
                    },
                    "severity": 1,
                    "message": "type error",
                    "source": "rustc"
                }]
            }
        });
        let frame = encode_frame(&serde_json::to_vec(&diag_notif).unwrap());
        server_write.write_all(&frame).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let diags = client.get_diagnostics(Some(&target_file)).await;
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].1[0].message, "type error");
    }

    #[tokio::test]
    async fn test_client_notify_did_open() {
        let (client_read, _server_write) = tokio::io::duplex(4096);
        let (mut server_read, client_write) = tokio::io::duplex(4096);
        let temp_dir = std::env::temp_dir();
        let client = LspClient::new(client_read, client_write, temp_dir.clone());
        let file = temp_dir.join("main.rs");
        client.notify_did_open(&file, "fn main() {}").await.unwrap();
        let mut buf = Vec::new();
        let frame = read_frame(&mut server_read, &mut buf).await.unwrap().unwrap();
        let val: serde_json::Value = serde_json::from_slice(&frame).unwrap();
        assert_eq!(val["method"], "textDocument/didOpen");
        assert_eq!(val["params"]["textDocument"]["languageId"], "rust");
    }

    #[test]
    fn test_detect_language_id() {
        let rs = detect_language_id("rs");
        let ts = detect_language_id("ts");
        let js = detect_language_id("js");
        let go = detect_language_id("go");
        let py = detect_language_id("py");
        let fallback = detect_language_id("xyz");
        assert_eq!(
            format!("{rs}:{ts}:{js}:{go}:{py}:{fallback}"),
            "rust:typescript:javascript:go:python:plaintext"
        );
    }
}
