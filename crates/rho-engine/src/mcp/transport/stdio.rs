use super::super::process::McpChildHandle;
use super::super::types::{JsonRpcError, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
use super::Inbound;
use super::route::{Routed, reply_to_server_request, route_message, server_error};
use crate::locking::lock;
use rho_harness_core::error::{AppError, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::oneshot;

type ResponseTx = oneshot::Sender<std::result::Result<Value, JsonRpcError>>;

pub struct StdioTransport {
    stdin: Arc<tokio::sync::Mutex<ChildStdin>>,
    next_id: AtomicI64,
    pending: Arc<Mutex<BTreeMap<i64, ResponseTx>>>,
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

fn cancel_pending_on_close(pending: &Mutex<BTreeMap<i64, ResponseTx>>) {
    lock(pending).clear();
}

fn spawn_stdout_reader(
    stdout: ChildStdout,
    stdin: Arc<tokio::sync::Mutex<ChildStdin>>,
    pending: Arc<Mutex<BTreeMap<i64, ResponseTx>>>,
    inbound: Inbound,
) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            if let Some(reply) = handle_stdout_line(&line, &pending, &inbound)
                && let Err(err) = write_json_line(&stdin, &reply).await
            {
                tracing::warn!("failed to answer MCP server request: {err}");
            }
        }
        cancel_pending_on_close(&pending);
    });
}

async fn write_json_line(stdin: &tokio::sync::Mutex<ChildStdin>, message: &impl serde::Serialize) -> Result<()> {
    let mut json = serde_json::to_string(message).map_err(|e| AppError::Mcp(e.to_string()))?;
    json.push('\n');
    let mut lock = stdin.lock().await;
    lock.write_all(json.as_bytes())
        .await
        .map_err(|e| AppError::McpNotDelivered(format!("failed to write to MCP stdin: {e}")))?;
    lock.flush()
        .await
        .map_err(|e| AppError::McpConnectionLost(format!("failed to flush MCP stdin: {e}")))
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
        let inbound = Inbound::default();
        spawn_stdout_reader(stdout, Arc::clone(&stdin), Arc::clone(&pending), inbound.clone());

        Arc::new(Self {
            stdin,
            next_id: AtomicI64::new(1),
            pending,
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
}
