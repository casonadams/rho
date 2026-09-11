pub mod http;
pub mod stdio;

pub use http::HttpTransport;
pub use stdio::StdioTransport;

use super::process::McpChildHandle;
use rho_harness_core::config::McpTransportKind;
use rho_harness_core::error::Result;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::{ChildStdin, ChildStdout};

pub enum TransportKind {
    Stdio(Arc<StdioTransport>),
    Http(Arc<HttpTransport>),
}

pub struct McpTransport {
    inner: TransportKind,
}

impl McpTransport {
    pub fn new(stdin: ChildStdin, stdout: ChildStdout, handle: McpChildHandle) -> Arc<Self> {
        Self::new_stdio(stdin, stdout, handle, None)
    }

    pub fn new_stdio(
        stdin: ChildStdin,
        stdout: ChildStdout,
        handle: McpChildHandle,
        timeout: Option<Duration>,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner: TransportKind::Stdio(StdioTransport::new(stdin, stdout, handle, timeout)),
        })
    }

    pub fn new_http(
        url: impl Into<String>,
        kind: McpTransportKind,
        headers: BTreeMap<String, String>,
        timeout: Option<Duration>,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner: TransportKind::Http(HttpTransport::new(url, kind, headers, timeout)),
        })
    }

    pub async fn request(&self, method: &str, params: Option<Value>) -> Result<Value> {
        match &self.inner {
            TransportKind::Stdio(s) => s.request(method, params).await,
            TransportKind::Http(h) => h.request(method, params).await,
        }
    }

    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<()> {
        match &self.inner {
            TransportKind::Stdio(s) => s.notify(method, params).await,
            TransportKind::Http(h) => h.notify(method, params).await,
        }
    }

    pub fn last_stderr(&self) -> String {
        match &self.inner {
            TransportKind::Stdio(s) => s.last_stderr(),
            TransportKind::Http(_) => String::new(),
        }
    }
}

#[cfg(all(test, unix))]
mod tests;
