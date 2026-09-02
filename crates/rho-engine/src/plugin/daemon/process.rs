use super::resolve::resolve_executable;
use crate::plugin::host::HostDispatcher;
use crate::plugin::protocol::{JsonRpcRequest, JsonRpcResponse};
use rho_harness_core::config::PluginConfig;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Child;
use tokio::sync::{Mutex, mpsc, oneshot};

pub type PendingResponses = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<JsonRpcResponse, String>>>>>;

pub struct DaemonSpawnArgs<'a> {
    pub name: &'a str,
    pub config: &'a PluginConfig,
    pub working_dir: &'a Path,
    pub dispatcher: Arc<HostDispatcher>,
}

struct StdoutReaderContext {
    pending: PendingResponses,
    dispatcher: Arc<HostDispatcher>,
    stdin_tx: mpsc::Sender<String>,
}

pub struct DaemonProcess {
    pub name: String,
    next_id: AtomicU64,
    stdin_tx: mpsc::Sender<String>,
    pending: PendingResponses,
    subscriptions: HashSet<String>,
    _child: Arc<Mutex<Child>>,
}

impl DaemonProcess {
    pub async fn spawn(args: DaemonSpawnArgs<'_>) -> Result<Self, String> {
        let (program, cmd_args) = resolve_executable(args.config, args.working_dir)?;
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(cmd_args)
            .current_dir(args.working_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn {}: {e}", args.name))?;
        let child_stdin = child.stdin.take().ok_or("Failed to open child stdin")?;
        let child_stdout = child.stdout.take().ok_or("Failed to open child stdout")?;
        let pending: PendingResponses = Arc::new(Mutex::new(HashMap::new()));
        let (stdin_tx, stdin_rx) = mpsc::channel::<String>(64);

        spawn_stdin_writer(child_stdin, stdin_rx);
        spawn_stdout_reader(
            child_stdout,
            StdoutReaderContext {
                pending: pending.clone(),
                dispatcher: args.dispatcher,
                stdin_tx: stdin_tx.clone(),
            },
        );

        Ok(Self {
            name: args.name.to_string(),
            next_id: AtomicU64::new(1),
            stdin_tx,
            pending,
            subscriptions: HashSet::new(),
            _child: Arc::new(Mutex::new(child)),
        })
    }

    pub fn with_subscriptions(mut self, subs: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.subscriptions = subs.into_iter().map(Into::into).collect();
        self
    }

    pub fn subscribes_to(&self, event: &str) -> bool {
        self.subscriptions.is_empty() || self.subscriptions.contains("all") || self.subscriptions.contains(event)
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<JsonRpcResponse, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let req = JsonRpcRequest::new(id, method, params);
        let json_line = serde_json::to_string(&req).map_err(|e| e.to_string())?;

        let (tx, rx) = oneshot::channel();
        {
            let mut map = self.pending.lock().await;
            map.insert(id, tx);
        }

        self.stdin_tx
            .send(json_line)
            .await
            .map_err(|e| format!("Failed to send to plugin stdin: {e}"))?;

        match tokio::time::timeout(Duration::from_secs(5), rx).await {
            Ok(Ok(response)) => response,
            Ok(Err(_)) => Err("Plugin response channel closed".to_string()),
            Err(_) => {
                let mut map = self.pending.lock().await;
                map.remove(&id);
                Err("Plugin call timed out".to_string())
            }
        }
    }
}

fn spawn_stdin_writer(mut stdin: tokio::process::ChildStdin, mut rx: mpsc::Receiver<String>) {
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

fn spawn_stdout_reader(stdout: tokio::process::ChildStdout, ctx: StdoutReaderContext) {
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
                let resp = ctx.dispatcher.dispatch(req).await;
                if let Ok(resp_json) = serde_json::to_string(&resp) {
                    let _ = ctx.stdin_tx.send(resp_json).await;
                }
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
