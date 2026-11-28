pub mod http;
mod route;
pub mod stdio;

pub use http::HttpTransport;
pub use stdio::StdioTransport;

use super::oauth::McpOAuth;
use super::process::McpChildHandle;
use rho_harness_core::config::McpTransportKind;
use rho_harness_core::error::{AppError, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::process::{ChildStdin, ChildStdout};

pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_TOOL_CALL_TIMEOUT: Duration = Duration::from_secs(600);
pub const MAX_PROGRESS_EXTENSION: Duration = Duration::from_secs(60 * 60);
const PROGRESS_NOTIFICATION: &str = "notifications/progress";

/// Receives the method name of every JSON-RPC notification the server sends.
pub type TransportNotificationHook = Arc<dyn Fn(&str) + Send + Sync>;
pub(crate) type NotificationSlot = Arc<std::sync::OnceLock<TransportNotificationHook>>;

/// Server-to-client notification plumbing shared by both transports.
#[derive(Clone, Default)]
pub(crate) struct Inbound {
    hook: NotificationSlot,
    progress: Arc<AtomicU64>,
}

impl Inbound {
    pub(crate) fn dispatch(&self, method: &str) {
        if method == PROGRESS_NOTIFICATION {
            self.progress.fetch_add(1, Ordering::SeqCst);
        }
        if let Some(hook) = self.hook.get() {
            hook(method);
        }
    }

    fn progress_count(&self) -> u64 {
        self.progress.load(Ordering::SeqCst)
    }
}

#[derive(Clone)]
pub enum TransportKind {
    Stdio(Arc<StdioTransport>),
    Http(Arc<HttpTransport>),
}

impl TransportKind {
    fn inbound(&self) -> &Inbound {
        match self {
            Self::Stdio(s) => s.inbound(),
            Self::Http(h) => h.inbound(),
        }
    }

    fn next_id(&self) -> i64 {
        match self {
            Self::Stdio(s) => s.next_id(),
            Self::Http(h) => h.next_id(),
        }
    }

    async fn send_request(&self, id: i64, method: &str, params: Option<Value>) -> Result<Value> {
        match self {
            Self::Stdio(s) => s.send_request(id, method, params).await,
            Self::Http(h) => h.send_request(id, method, params).await,
        }
    }

    async fn notify(&self, method: &str, params: Option<Value>) -> Result<()> {
        match self {
            Self::Stdio(s) => s.notify(method, params).await,
            Self::Http(h) => h.notify(method, params).await,
        }
    }

    fn describe(&self) -> String {
        match self {
            Self::Stdio(_) => "stdio server".to_string(),
            Self::Http(h) => format!("'{}'", h.url()),
        }
    }
}

pub fn request_timeout(method: &str, configured: Option<Duration>) -> Duration {
    configured.unwrap_or(if method == "tools/call" {
        DEFAULT_TOOL_CALL_TIMEOUT
    } else {
        DEFAULT_REQUEST_TIMEOUT
    })
}

/// Waits for `fut`, allowing another `idle` window whenever progress arrived during the last one.
async fn wait_with_progress<F: Future>(fut: F, idle: Duration, inbound: &Inbound) -> Option<F::Output> {
    tokio::pin!(fut);
    let started = tokio::time::Instant::now();
    let mut seen = inbound.progress_count();
    loop {
        if let Ok(output) = tokio::time::timeout(idle, &mut fut).await {
            return Some(output);
        }
        let now = inbound.progress_count();
        if now == seen || started.elapsed() >= MAX_PROGRESS_EXTENSION {
            return None;
        }
        seen = now;
    }
}

/// Tells the server to stop work on a request that was abandoned by timeout or by dropping its future.
struct CancelOnDrop {
    inner: TransportKind,
    id: i64,
    armed: bool,
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if let TransportKind::Stdio(s) = &self.inner {
            s.forget(self.id);
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let (inner, id) = (self.inner.clone(), self.id);
        runtime.spawn(async move {
            let params = serde_json::json!({ "requestId": id, "reason": "client cancelled the request" });
            if let Err(err) = inner.notify("notifications/cancelled", Some(params)).await {
                tracing::debug!("best-effort MCP cancellation of abandoned request {id} not delivered: {err}");
            }
        });
    }
}

fn with_progress_token(method: &str, params: Option<Value>, id: i64) -> Option<Value> {
    if method != "tools/call" {
        return params;
    }
    let mut params = params.unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    if let Some(obj) = params.as_object_mut() {
        obj.entry("_meta")
            .or_insert_with(|| Value::Object(serde_json::Map::new()))
            .as_object_mut()
            .map(|meta| meta.insert("progressToken".to_string(), Value::from(id)));
    }
    Some(params)
}

pub struct McpTransport {
    inner: TransportKind,
    timeout: Option<Duration>,
}

impl McpTransport {
    pub fn new_stdio(
        stdin: ChildStdin,
        stdout: ChildStdout,
        handle: McpChildHandle,
        timeout: Option<Duration>,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner: TransportKind::Stdio(StdioTransport::new(stdin, stdout, handle)),
            timeout,
        })
    }

    pub fn new_http(
        url: impl Into<String>,
        kind: McpTransportKind,
        headers: BTreeMap<String, String>,
        timeout: Option<Duration>,
        oauth: Option<Arc<McpOAuth>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner: TransportKind::Http(HttpTransport::new(url, kind, headers, oauth)),
            timeout,
        })
    }

    pub fn set_notification_hook(&self, hook: TransportNotificationHook) {
        let _ = self.inner.inbound().hook.set(hook);
    }

    pub async fn request(&self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.inner.next_id();
        let params = with_progress_token(method, params, id);
        let mut guard = CancelOnDrop {
            inner: self.inner.clone(),
            id,
            armed: method != "initialize",
        };
        let idle = request_timeout(method, self.timeout);
        let sent = self.inner.send_request(id, method, params);
        let Some(result) = wait_with_progress(sent, idle, self.inner.inbound()).await else {
            return Err(AppError::Mcp(format!(
                "MCP request '{method}' to {} timed out after {}s without progress",
                self.inner.describe(),
                idle.as_secs()
            )));
        };
        guard.armed = false;
        result
    }

    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<()> {
        self.inner.notify(method, params).await
    }
}

#[cfg(all(test, unix))]
mod tests;
