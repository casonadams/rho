use super::super::process::McpChildHandle;
use super::super::types::{JsonRpcError, JsonRpcIncoming, JsonRpcNotification, JsonRpcRequest};
use rho_harness_core::error::{AppError, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::oneshot;

pub const DEFAULT_MCP_TIMEOUT: Duration = Duration::from_secs(30);

type ResponseTx = oneshot::Sender<std::result::Result<Value, JsonRpcError>>;

pub struct StdioTransport {
    stdin: Arc<tokio::sync::Mutex<ChildStdin>>,
    next_id: AtomicI64,
    pending: Arc<Mutex<BTreeMap<i64, ResponseTx>>>,
    handle: McpChildHandle,
    timeout: Duration,
}

fn handle_stdout_line(line: &str, pending: &Mutex<BTreeMap<i64, ResponseTx>>) {
    if let Ok(JsonRpcIncoming::Response(resp)) = serde_json::from_str::<JsonRpcIncoming>(line)
        && let Some(id_num) = resp.id.as_i64()
        && let Some(tx) = pending.lock().unwrap().remove(&id_num)
    {
        let res = resp
            .error
            .map(Err)
            .unwrap_or_else(|| Ok(resp.result.unwrap_or(Value::Null)));
        let _ = tx.send(res);
    }
}

fn cancel_pending_on_close(pending: &Mutex<BTreeMap<i64, ResponseTx>>) {
    let mut guard = pending.lock().unwrap();
    for (_id, tx) in guard.split_off(&0) {
        let _ = tx.send(Err(JsonRpcError {
            code: -32000,
            message: "MCP process terminated unexpectedly".to_string(),
            data: None,
        }));
    }
}

fn spawn_stdout_reader(stdout: ChildStdout, pending: Arc<Mutex<BTreeMap<i64, ResponseTx>>>) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            if !line.trim().is_empty() {
                handle_stdout_line(&line, &pending);
            }
        }
        cancel_pending_on_close(&pending);
    });
}

async fn write_stdin_line(stdin: &tokio::sync::Mutex<ChildStdin>, json: String) -> Result<()> {
    let mut lock = stdin.lock().await;
    lock.write_all(json.as_bytes())
        .await
        .map_err(|e| AppError::Plugin(format!("Failed to write to MCP stdin: {e}")))?;
    lock.flush()
        .await
        .map_err(|e| AppError::Plugin(format!("Failed to flush MCP stdin: {e}")))
}

async fn await_mcp_response(
    rx: oneshot::Receiver<std::result::Result<Value, JsonRpcError>>,
    (pending, id): (&Mutex<BTreeMap<i64, ResponseTx>>, i64),
    method: &str,
    timeout: Duration,
) -> Result<Value> {
    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(Ok(val))) => Ok(val),
        Ok(Ok(Err(e))) => Err(AppError::Plugin(format!(
            "MCP error from {method}: {} (code {})",
            e.message, e.code
        ))),
        Ok(Err(_)) => Err(AppError::Plugin(format!(
            "MCP server closed stream while waiting for {method}"
        ))),
        Err(_) => {
            pending.lock().unwrap().remove(&id);
            Err(AppError::Plugin(format!("MCP request '{method}' timed out")))
        }
    }
}

impl StdioTransport {
    pub fn new(stdin: ChildStdin, stdout: ChildStdout, handle: McpChildHandle, timeout: Option<Duration>) -> Arc<Self> {
        let stdin = Arc::new(tokio::sync::Mutex::new(stdin));
        let pending = Arc::new(Mutex::new(BTreeMap::<i64, ResponseTx>::new()));
        spawn_stdout_reader(stdout, Arc::clone(&pending));

        Arc::new(Self {
            stdin,
            next_id: AtomicI64::new(1),
            pending,
            handle,
            timeout: timeout.unwrap_or(DEFAULT_MCP_TIMEOUT),
        })
    }

    pub async fn request(&self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);

        let req = JsonRpcRequest::new(id, method, params);
        let json = format!(
            "{}\n",
            serde_json::to_string(&req).map_err(|e| AppError::Plugin(e.to_string()))?
        );
        write_stdin_line(&self.stdin, json).await?;
        await_mcp_response(rx, (&self.pending, id), method, self.timeout).await
    }

    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<()> {
        let notif = JsonRpcNotification::new(method, params);
        let json = format!(
            "{}\n",
            serde_json::to_string(&notif).map_err(|e| AppError::Plugin(e.to_string()))?
        );
        write_stdin_line(&self.stdin, json).await
    }

    pub fn last_stderr(&self) -> String {
        self.handle.last_stderr()
    }
}
