use super::super::process::McpChildHandle;
use super::super::types::{JsonRpcError, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
use super::Inbound;
use super::route::{Routed, reply_to_server_request, route_message, server_error};
use crate::locking::lock;
use rho_harness_core::error::{AppError, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::oneshot;

const MAX_LINE_BYTES: usize = 32 * 1024 * 1024;
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

type ResponseTx = oneshot::Sender<std::result::Result<Value, JsonRpcError>>;

pub struct StdioTransport {
    stdin: Arc<tokio::sync::Mutex<ChildStdin>>,
    next_id: AtomicI64,
    pending: Arc<Mutex<BTreeMap<i64, ResponseTx>>>,
    closed: Arc<AtomicBool>,
    handle: McpChildHandle,
    inbound: Inbound,
}

fn handle_stdout_line(
    line: &str,
    pending: &Mutex<BTreeMap<i64, ResponseTx>>,
    inbound: &Inbound,
) -> Option<JsonRpcResponse> {
    let message = serde_json::from_str::<Value>(line).ok()?;
    match route_message(message)? {
        Routed::Notification(method) => inbound.dispatch(&method),
        Routed::Request { id, method } => return Some(reply_to_server_request(id, &method)),
        Routed::Response(id, outcome) => {
            if let Some(tx) = id.as_i64().and_then(|id| lock(pending).remove(&id)) {
                let _ = tx.send(outcome);
            }
        }
    }
    None
}

fn cancel_pending_on_close(pending: &Mutex<BTreeMap<i64, ResponseTx>>, closed: &AtomicBool) {
    closed.store(true, Ordering::SeqCst);
    lock(pending).clear();
}

/// The next line without its `\n`, or `None` at end of input; a line longer than `max` bytes is an error
/// rather than an ever-growing buffer.
async fn next_capped_line<R: AsyncBufRead + Unpin>(reader: &mut R, max: usize) -> std::io::Result<Option<String>> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok((!line.is_empty()).then(|| String::from_utf8_lossy(&line).into_owned()));
        }
        let newline = available.iter().position(|b| *b == b'\n');
        let chunk = &available[..newline.unwrap_or(available.len())];
        if line.len() + chunk.len() > max {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("MCP server sent a line over {max} bytes"),
            ));
        }
        line.extend_from_slice(chunk);
        let used = newline.map_or(chunk.len(), |at| at + 1);
        reader.consume(used);
        if newline.is_some() {
            return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
        }
    }
}

fn spawn_stdout_reader(
    stdout: ChildStdout,
    stdin: Arc<tokio::sync::Mutex<ChildStdin>>,
    pending: Arc<Mutex<BTreeMap<i64, ResponseTx>>>,
    closed: Arc<AtomicBool>,
    inbound: Inbound,
) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        loop {
            let line = match next_capped_line(&mut reader, MAX_LINE_BYTES).await {
                Ok(Some(line)) => line,
                Ok(None) => break,
                Err(err) => {
                    tracing::warn!("closing MCP stdio connection: {err}");
                    break;
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            if let Some(reply) = handle_stdout_line(&line, &pending, &inbound)
                && let Err(err) = write_json_line(&stdin, &reply).await
            {
                tracing::warn!("failed to answer MCP server request: {err}");
            }
        }
        cancel_pending_on_close(&pending, &closed);
    });
}

/// Writes one JSON line within `WRITE_TIMEOUT`: not delivered when the pipe stays locked by another
/// writer, connection lost when the server stops draining its stdin mid-message.
async fn write_json_line<W: AsyncWrite + Unpin>(
    stdin: &tokio::sync::Mutex<W>,
    message: &impl serde::Serialize,
) -> Result<()> {
    let mut json = serde_json::to_string(message).map_err(|e| AppError::Mcp(e.to_string()))?;
    json.push('\n');
    let deadline = tokio::time::Instant::now() + WRITE_TIMEOUT;
    let mut lock = tokio::time::timeout_at(deadline, stdin.lock())
        .await
        .map_err(|_| AppError::McpNotDelivered("MCP stdin stayed busy past the write timeout".into()))?;
    let write = async {
        lock.write_all(json.as_bytes())
            .await
            .map_err(|e| AppError::McpNotDelivered(format!("failed to write to MCP stdin: {e}")))?;
        lock.flush()
            .await
            .map_err(|e| AppError::McpConnectionLost(format!("failed to flush MCP stdin: {e}")))
    };
    tokio::time::timeout_at(deadline, write).await.map_err(|_| {
        AppError::McpConnectionLost(format!(
            "MCP server did not read its stdin within {}s",
            WRITE_TIMEOUT.as_secs()
        ))
    })?
}

pub(super) async fn await_mcp_response(
    rx: oneshot::Receiver<std::result::Result<Value, JsonRpcError>>,
    method: &str,
) -> Result<Value> {
    match rx.await {
        Ok(Ok(val)) => Ok(val),
        Ok(Err(e)) => Err(server_error(method, &e)),
        Err(_) => Err(AppError::McpConnectionLost(format!(
            "MCP server process exited while waiting for {method}"
        ))),
    }
}

impl StdioTransport {
    pub fn new(stdin: ChildStdin, stdout: ChildStdout, handle: McpChildHandle) -> Arc<Self> {
        let stdin = Arc::new(tokio::sync::Mutex::new(stdin));
        let pending = Arc::new(Mutex::new(BTreeMap::<i64, ResponseTx>::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let inbound = Inbound::default();
        spawn_stdout_reader(
            stdout,
            Arc::clone(&stdin),
            Arc::clone(&pending),
            Arc::clone(&closed),
            inbound.clone(),
        );

        Arc::new(Self {
            stdin,
            next_id: AtomicI64::new(1),
            pending,
            closed,
            handle,
            inbound,
        })
    }

    pub(crate) fn inbound(&self) -> &Inbound {
        &self.inbound
    }

    pub(crate) fn next_id(&self) -> i64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    pub(crate) fn forget(&self, id: i64) {
        lock(&self.pending).remove(&id);
    }

    pub(crate) async fn send_request(&self, id: i64, method: &str, params: Option<Value>) -> Result<Value> {
        let (tx, rx) = oneshot::channel();
        lock(&self.pending).insert(id, tx);
        if self.closed.load(Ordering::SeqCst) {
            self.forget(id);
            return Err(AppError::McpNotDelivered(
                "MCP stdio connection is closed (server exited or sent an oversized line)".into(),
            ));
        }

        write_json_line(&self.stdin, &JsonRpcRequest::new(id, method, params)).await?;
        await_mcp_response(rx, method).await
    }

    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<()> {
        write_json_line(&self.stdin, &JsonRpcNotification::new(method, params)).await
    }

    pub fn last_stderr(&self) -> String {
        self.handle.last_stderr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdout_lines_route_responses_notifications_and_server_requests() {
        let pending = Mutex::new(BTreeMap::new());
        let (tx, mut rx) = oneshot::channel();
        pending.lock().unwrap().insert(1, tx);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let inbound = Inbound::default();
        let sink = Arc::clone(&seen);
        let _ = inbound.hook.set(Arc::new(move |method: &str| {
            sink.lock().unwrap().push(method.to_string())
        }));

        let lines = [
            r#"{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"roots/list"}"#,
            "not json",
            r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#,
        ];
        let replies: Vec<_> = lines
            .into_iter()
            .filter_map(|line| handle_stdout_line(line, &pending, &inbound))
            .collect();

        assert_eq!(*seen.lock().unwrap(), ["notifications/tools/list_changed"]);
        assert_eq!(
            serde_json::to_value(replies).unwrap(),
            serde_json::json!([{ "jsonrpc": "2.0", "id": 1, "error": { "code": -32601, "message": "Method not found: roots/list" } }])
        );
        assert_eq!(rx.try_recv().unwrap().unwrap(), serde_json::json!({ "ok": true }));
    }

    #[tokio::test]
    async fn stdout_lines_are_split_and_an_oversized_line_is_an_error() {
        let input: &[u8] = b"abc\n\ndefgh\ntail";
        let mut reader = BufReader::with_capacity(2, input);
        let mut lines = Vec::new();
        while let Some(line) = next_capped_line(&mut reader, 5).await.unwrap() {
            lines.push(line);
        }
        assert_eq!(lines, ["abc", "", "defgh", "tail"]);

        let mut reader = BufReader::with_capacity(2, &b"abcdefgh\nok\n"[..]);
        let err = next_capped_line(&mut reader, 5).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[tokio::test(start_paused = true)]
    async fn a_server_that_stops_reading_stdin_loses_the_connection_after_the_write_timeout() {
        let (writer, _unread) = tokio::io::duplex(8);
        let stdin = tokio::sync::Mutex::new(writer);
        let err = write_json_line(&stdin, &"a message longer than the pipe buffer")
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::McpConnectionLost(_)), "{err}");
    }

    #[tokio::test(start_paused = true)]
    async fn a_write_stuck_behind_another_writer_is_not_delivered() {
        let (writer, _unread) = tokio::io::duplex(1024);
        let stdin = tokio::sync::Mutex::new(writer);
        let _held = stdin.lock().await;
        let err = write_json_line(&stdin, &"queued").await.unwrap_err();
        assert!(matches!(err, AppError::McpNotDelivered(_)), "{err}");
    }
}
