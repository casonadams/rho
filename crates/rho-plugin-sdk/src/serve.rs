use super::context::HostContext;
use super::types::{Flow, StepEvent};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, mpsc, oneshot};

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

fn spawn_writer<W: tokio::io::AsyncWrite + Unpin + Send + 'static>(mut writer: W, mut out_rx: mpsc::Receiver<String>) {
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
}

async fn handle_rpc_response(pending_rpc: &Mutex<HashMap<u64, oneshot::Sender<Value>>>, val: &Value) -> bool {
    let Some(id) = val.get("id").and_then(Value::as_u64) else {
        return false;
    };
    if val.get("method").is_some() {
        return false;
    }
    let mut map = pending_rpc.lock().await;
    if let Some(tx) = map.remove(&id) {
        let res = val.get("result").cloned().unwrap_or(Value::Null);
        let _ = tx.send(res);
    }
    true
}

async fn handle_initialize<P: Plugin>(plugin: &P, req_id: Value, out_tx: &mpsc::Sender<String>) {
    let res = json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "result": {
            "protocolVersion": "2024-11-05",
            "subscribes": plugin.subscriptions(),
            "serverInfo": { "name": plugin.name() }
        }
    });
    let _ = out_tx.send(res.to_string()).await;
}

async fn dispatch_event<P: Plugin + 'static>(plugin: Arc<P>, ctx: HostContext, (req_id, val): (Value, &Value)) {
    let params = val.get("params").cloned().unwrap_or(Value::Null);
    let Ok(event) = serde_json::from_value::<StepEvent>(params) else {
        let err = json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "error": { "code": -32602, "message": "Invalid event parameters" }
        });
        let _ = ctx.out_tx.send(err.to_string()).await;
        return;
    };
    let out_tx = ctx.out_tx.clone();
    tokio::spawn(async move {
        let flow = plugin.on_event(event, &ctx).await;
        let resp = json!({ "jsonrpc": "2.0", "id": req_id, "result": flow });
        let _ = out_tx.send(resp.to_string()).await;
    });
}

async fn handle_request<P: Plugin + 'static>(plugin: Arc<P>, ctx: HostContext, val: Value) {
    let Some(method) = val.get("method").and_then(Value::as_str) else {
        return;
    };
    let req_id = val.get("id").cloned().unwrap_or(Value::Null);
    if method == "initialize" {
        handle_initialize(&*plugin, req_id, &ctx.out_tx).await;
    } else {
        dispatch_event(plugin, ctx, (req_id, &val)).await;
    }
}

async fn process_line<P: Plugin + 'static>(plugin: Arc<P>, ctx: &HostContext, line: &str) {
    let Ok(val) = serde_json::from_str::<Value>(line.trim()) else {
        return;
    };
    if !handle_rpc_response(&ctx.pending_rpc, &val).await {
        handle_request(plugin, ctx.clone(), val).await;
    }
}

pub async fn serve_stdio<P: Plugin + 'static, R, W>(plugin: P, reader: R, writer: W)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let plugin = Arc::new(plugin);
    let (out_tx, out_rx) = mpsc::channel::<String>(64);
    let ctx = HostContext {
        out_tx,
        pending_rpc: Arc::new(Mutex::new(HashMap::new())),
        next_id: Arc::new(AtomicU64::new(1000)),
    };
    spawn_writer(writer, out_rx);

    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        process_line(plugin.clone(), &ctx, &line).await;
    }
}
