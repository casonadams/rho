use super::context::HostContext;
use super::types::{Flow, StepEvent};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, mpsc};

#[async_trait]
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str {
        "rho-plugin"
    }

    fn subscriptions(&self) -> Vec<String> {
        vec!["tool_call".to_string(), "invalid_tool_call".to_string()]
    }

    async fn on_event(&self, event: StepEvent, ctx: &HostContext) -> Flow {
        let _ = (event, ctx);
        Flow::cont()
    }
}

pub async fn serve<P: Plugin + 'static>(plugin: P) {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    serve_stdio(plugin, stdin, stdout).await;
}

pub async fn serve_stdio<P: Plugin + 'static, R, W>(plugin: P, reader: R, mut writer: W)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let plugin = Arc::new(plugin);
    let (out_tx, mut out_rx) = mpsc::channel::<String>(64);
    let pending_rpc = Arc::new(Mutex::new(HashMap::new()));
    let next_id = Arc::new(AtomicU64::new(1000));

    let ctx = HostContext {
        out_tx: out_tx.clone(),
        pending_rpc: pending_rpc.clone(),
        next_id,
    };

    tokio::spawn(async move {
        while let Some(line) = out_rx.recv().await {
            if writer.write_all(line.as_bytes()).await.is_err()
                || writer.write_all(b"\n").await.is_err()
                || writer.flush().await.is_err()
            {
                break;
            }
        }
    });

    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(val) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };

        if let Some(id) = val.get("id").and_then(Value::as_u64)
            && val.get("method").is_none()
        {
            let mut map = pending_rpc.lock().await;
            if let Some(tx) = map.remove(&id) {
                let res = val.get("result").cloned().unwrap_or(Value::Null);
                let _ = tx.send(res);
            }
            continue;
        }

        let Some(method) = val.get("method").and_then(Value::as_str) else {
            continue;
        };
        let req_id = val.get("id").cloned().unwrap_or(Value::Null);

        if method == "initialize" {
            let res = json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "subscribes": plugin.subscriptions(),
                    "serverInfo": {
                        "name": plugin.name(),
                    }
                }
            });
            let _ = out_tx.send(res.to_string()).await;
            continue;
        }

        let params = val.get("params").cloned().unwrap_or(Value::Null);
        let Ok(event) = serde_json::from_value::<StepEvent>(params) else {
            let err = json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "error": {
                    "code": -32602,
                    "message": "Invalid event parameters"
                }
            });
            let _ = out_tx.send(err.to_string()).await;
            continue;
        };

        let plugin = plugin.clone();
        let ctx = ctx.clone();
        let out_tx = out_tx.clone();

        tokio::spawn(async move {
            let flow = plugin.on_event(event, &ctx).await;
            let resp = json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": flow
            });
            let _ = out_tx.send(resp.to_string()).await;
        });
    }
}
