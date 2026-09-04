use super::process::McpChildHandle;
use super::types::{JsonRpcError, JsonRpcIncoming, JsonRpcNotification, JsonRpcRequest};
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

pub struct McpTransport {
    stdin: Arc<tokio::sync::Mutex<ChildStdin>>,
    next_id: AtomicI64,
    pending: Arc<Mutex<BTreeMap<i64, ResponseTx>>>,
    handle: McpChildHandle,
}

impl McpTransport {
    pub fn new(stdin: ChildStdin, stdout: ChildStdout, handle: McpChildHandle) -> Arc<Self> {
        let stdin = Arc::new(tokio::sync::Mutex::new(stdin));
        let pending = Arc::new(Mutex::new(BTreeMap::<i64, ResponseTx>::new()));

        let pending_clone = Arc::clone(&pending);
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(JsonRpcIncoming::Response(resp)) = serde_json::from_str::<JsonRpcIncoming>(&line)
                    && let Some(id_num) = resp.id.as_i64()
                    && let Some(tx) = pending_clone.lock().unwrap().remove(&id_num)
                {
                    if let Some(err) = resp.error {
                        let _ = tx.send(Err(err));
                    } else {
                        let _ = tx.send(Ok(resp.result.unwrap_or(Value::Null)));
                    }
                }
            }
            // Stream closed: cancel any remaining pending requests
            let mut guard = pending_clone.lock().unwrap();
            for (_id, tx) in guard.split_off(&0) {
                let _ = tx.send(Err(JsonRpcError {
                    code: -32000,
                    message: "MCP process terminated unexpectedly".to_string(),
                    data: None,
                }));
            }
        });

        Arc::new(Self {
            stdin,
            next_id: AtomicI64::new(1),
            pending,
            handle,
        })
    }

    pub async fn request(&self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);

        let req = JsonRpcRequest::new(id, method, params);
        let mut json = serde_json::to_string(&req).map_err(|e| AppError::Plugin(e.to_string()))?;
        json.push('\n');

        {
            let mut stdin = self.stdin.lock().await;
            stdin
                .write_all(json.as_bytes())
                .await
                .map_err(|e| AppError::Plugin(format!("Failed to write to MCP stdin: {e}")))?;
            stdin
                .flush()
                .await
                .map_err(|e| AppError::Plugin(format!("Failed to flush MCP stdin: {e}")))?;
        }

        match tokio::time::timeout(DEFAULT_MCP_TIMEOUT, rx).await {
            Ok(Ok(Ok(val))) => Ok(val),
            Ok(Ok(Err(rpc_err))) => Err(AppError::Plugin(format!(
                "MCP error from {method}: {} (code {})",
                rpc_err.message, rpc_err.code
            ))),
            Ok(Err(_closed)) => Err(AppError::Plugin(format!(
                "MCP server closed stream while waiting for {method}"
            ))),
            Err(_timeout) => {
                self.pending.lock().unwrap().remove(&id);
                Err(AppError::Plugin(format!("MCP request '{method}' timed out")))
            }
        }
    }

    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<()> {
        let notif = JsonRpcNotification::new(method, params);
        let mut json = serde_json::to_string(&notif).map_err(|e| AppError::Plugin(e.to_string()))?;
        json.push('\n');

        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(json.as_bytes())
            .await
            .map_err(|e| AppError::Plugin(format!("Failed to write notification to MCP stdin: {e}")))?;
        stdin
            .flush()
            .await
            .map_err(|e| AppError::Plugin(format!("Failed to flush MCP stdin: {e}")))?;
        Ok(())
    }

    pub fn last_stderr(&self) -> String {
        self.handle.last_stderr()
    }
}

#[cfg(all(test, unix))]
mod tests;
