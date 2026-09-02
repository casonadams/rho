use super::process::StdoutReaderContext;
use crate::plugin::protocol::{JsonRpcRequest, JsonRpcResponse};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

pub fn spawn_stdin_writer(mut stdin: tokio::process::ChildStdin, mut rx: mpsc::Receiver<String>) {
    tokio::spawn(async move {
        while let Some(line) = rx.recv().await {
            if stdin.write_all(line.as_bytes()).await.is_err()
                || stdin.write_all(b"\n").await.is_err()
                || stdin.flush().await.is_err()
            {
                break;
            }
        }
    });
}

pub fn spawn_stdout_reader(stdout: tokio::process::ChildStdout, ctx: StdoutReaderContext) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(val) = serde_json::from_str::<Value>(trimmed) else {
                continue;
            };

            if val.get("method").is_some() {
                let Ok(req) = serde_json::from_value::<JsonRpcRequest>(val) else {
                    continue;
                };
                let dispatcher = ctx.dispatcher.clone();
                let stdin_tx = ctx.stdin_tx.clone();
                tokio::spawn(async move {
                    let resp = dispatcher.dispatch(req).await;
                    if let Ok(resp_json) = serde_json::to_string(&resp) {
                        let _ = stdin_tx.send(resp_json).await;
                    }
                });
            } else if let Some(id) = val.get("id").and_then(Value::as_u64) {
                let mut map = ctx.pending.lock().await;
                if let Some(tx) = map.remove(&id) {
                    let resp =
                        serde_json::from_value::<JsonRpcResponse>(val).map_err(|e| format!("Malformed response: {e}"));
                    let _ = tx.send(resp);
                }
            }
        }
    });
}
