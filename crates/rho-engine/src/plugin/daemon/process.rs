use super::pump::{spawn_stdin_writer, spawn_stdout_reader};
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
use tokio::sync::{Mutex, mpsc, oneshot};

pub type PendingResponses = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<JsonRpcResponse, String>>>>>;

pub struct DaemonSpawnArgs<'a> {
    pub name: &'a str,
    pub config: &'a PluginConfig,
    pub working_dir: &'a Path,
    pub dispatcher: Arc<HostDispatcher>,
}

pub struct StdoutReaderContext {
    pub pending: PendingResponses,
    pub dispatcher: Arc<HostDispatcher>,
    pub stdin_tx: mpsc::Sender<String>,
}

pub struct DaemonProcess {
    pub name: String,
    next_id: AtomicU64,
    stdin_tx: mpsc::Sender<String>,
    pending: PendingResponses,
    subscriptions: HashSet<String>,
    _guard: Arc<Mutex<crate::process::ProcessTreeGuard>>,
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
        cmd.kill_on_drop(true);
        crate::process::isolate_group(&mut cmd);

        let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn {}: {e}", args.name))?;
        let child_stdin = child.stdin.take().ok_or("Failed to open child stdin")?;
        let child_stdout = child.stdout.take().ok_or("Failed to open child stdout")?;
        let guard = crate::process::ProcessTreeGuard::new(child);
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
            _guard: Arc::new(Mutex::new(guard)),
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

        match tokio::time::timeout(Duration::from_secs(600), rx).await {
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
