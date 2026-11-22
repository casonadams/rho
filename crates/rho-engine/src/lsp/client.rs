//! One running language server: initialization, document sync, readiness, and diagnostics.

use super::rpc::{BoxedReader, BoxedWriter, Connection, ServerMessages};
pub use super::state::{ClientState, Diagnostic};
use super::watch::FileChange;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::time::Instant;

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(60);
/// How long an unversioned diagnostics publish must stay unchanged before it counts as final.
pub const DIAGNOSTICS_SETTLE: Duration = Duration::from_millis(250);
/// After initialization, how long a server that reports no progress counts as still starting.
pub const STARTUP_GRACE: Duration = Duration::from_millis(500);
/// How long to wait for work a change starts on its own (e.g. a check run on save) to show up.
pub const FOLLOW_UP_GRACE: Duration = Duration::from_millis(200);
/// How long a server gets to answer `shutdown` before it is dropped without ceremony.
pub const SHUTDOWN_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a server gets to exit after `exit` before its process is killed.
pub const EXIT_WAIT: Duration = Duration::from_millis(500);
/// Documents held open per server; opening more closes the least recently used.
pub const MAX_OPEN_DOCUMENTS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionEncoding {
    Utf8,
    Utf16,
}

pub fn file_uri(path: &Path) -> String {
    url::Url::from_file_path(path).map_or_else(|()| format!("file://{}", path.display()), |url| url.to_string())
}

pub fn uri_path(uri: &str) -> Option<PathBuf> {
    url::Url::parse(uri).ok()?.to_file_path().ok()
}

struct OpenDocument {
    version: i64,
    text: String,
    /// The diagnostics sequence number before the last change, so unchanged text still accepts
    /// the publishes that followed that change and nothing older.
    baseline: u64,
    used: u64,
}

#[derive(Default)]
struct Documents {
    open: HashMap<String, OpenDocument>,
    clock: u64,
}

impl Documents {
    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    /// The least recently used document other than `keep`, once more than the cap are open.
    fn evict_candidate(&self, keep: &str) -> Option<String> {
        if self.open.len() <= MAX_OPEN_DOCUMENTS {
            return None;
        }
        self.open
            .iter()
            .filter(|(uri, _)| uri.as_str() != keep)
            .min_by_key(|(_, doc)| doc.used)
            .map(|(uri, _)| uri.clone())
    }
}

/// What a sync sent: the document version, and the diagnostics sequence number before it, so a
/// later wait only accepts diagnostics published after the change.
#[derive(Debug, Clone, Copy)]
pub struct Synced {
    pub version: i64,
    baseline: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DiagnosticsOutcome {
    Ready(Vec<Diagnostic>),
    /// Diagnostics so far, while the server is still busy with the named work.
    Partial(Vec<Diagnostic>, String),
    /// Nothing reported yet, with what the server is busy with when known.
    Pending(Option<String>),
}

pub struct LspClient {
    pub name: String,
    pub root: PathBuf,
    connection: Arc<Connection>,
    state: Arc<ClientState>,
    capabilities: Value,
    encoding: PositionEncoding,
    documents: tokio::sync::Mutex<Documents>,
    started: Instant,
    last_used: Mutex<Instant>,
    process: Mutex<Option<Box<dyn Send + Sync>>>,
}

pub struct ClientSpec<'a> {
    pub name: &'a str,
    pub root: &'a Path,
    pub initialization_options: Option<Value>,
    pub settings: Option<Value>,
}

fn client_capabilities() -> Value {
    json!({
        "general": { "positionEncodings": ["utf-8", "utf-16"] },
        "workspace": {
            "configuration": true,
            "workspaceFolders": true,
            "applyEdit": false,
            "workspaceEdit": { "documentChanges": true },
            "symbol": { "dynamicRegistration": false },
            "diagnostics": { "refreshSupport": false },
            "didChangeWatchedFiles": { "dynamicRegistration": false }
        },
        "textDocument": {
            "synchronization": { "didSave": true, "dynamicRegistration": false },
            "publishDiagnostics": { "versionSupport": true, "relatedInformation": false },
            "diagnostic": { "dynamicRegistration": true },
            "hover": { "contentFormat": ["markdown", "plaintext"] },
            "definition": { "linkSupport": true },
            "typeDefinition": { "linkSupport": true },
            "implementation": { "linkSupport": true },
            "references": {},
            "documentSymbol": { "hierarchicalDocumentSymbolSupport": true },
            "rename": { "prepareSupport": false },
            "codeAction": {
                "dynamicRegistration": false,
                "codeActionLiteralSupport": {
                    "codeActionKind": { "valueSet": ["", "quickfix", "refactor", "refactor.extract", "refactor.inline", "refactor.rewrite", "source", "source.organizeImports"] }
                },
                "resolveSupport": { "properties": ["edit"] },
                "isPreferredSupport": true
            },
            "formatting": { "dynamicRegistration": false }
        },
        "window": { "workDoneProgress": true },
        "experimental": { "serverStatusNotification": true }
    })
}

impl LspClient {
    /// Runs the `initialize` handshake over an already connected stream.
    pub async fn start(
        spec: ClientSpec<'_>,
        reader: BoxedReader,
        writer: BoxedWriter,
        process: Option<Box<dyn Send + Sync>>,
    ) -> Result<Self, String> {
        let state = Arc::new(ClientState::new(spec.settings.clone(), spec.root));
        let connection = Connection::start(reader, writer, Arc::clone(&state) as Arc<dyn ServerMessages>);
        let params = json!({
            "processId": std::process::id(),
            "clientInfo": { "name": "rho", "version": env!("CARGO_PKG_VERSION") },
            "rootUri": file_uri(spec.root),
            "rootPath": spec.root.display().to_string(),
            "workspaceFolders": [state.workspace_folder.clone()],
            "capabilities": client_capabilities(),
            "initializationOptions": spec.initialization_options.unwrap_or(Value::Null),
        });
        let response = connection.request("initialize", params, INITIALIZE_TIMEOUT).await?;
        connection.notify("initialized", json!({}))?;
        if let Some(settings) = spec.settings {
            connection.notify("workspace/didChangeConfiguration", json!({ "settings": settings }))?;
        }
        let capabilities = response.get("capabilities").cloned().unwrap_or(Value::Null);
        let encoding = match capabilities.get("positionEncoding").and_then(Value::as_str) {
            Some("utf-8") => PositionEncoding::Utf8,
            _ => PositionEncoding::Utf16,
        };
        Ok(Self {
            name: spec.name.to_string(),
            root: spec.root.to_path_buf(),
            connection,
            state,
            capabilities,
            encoding,
            documents: tokio::sync::Mutex::new(Documents::default()),
            started: Instant::now(),
            last_used: Mutex::new(Instant::now()),
            process: Mutex::new(process),
        })
    }

    pub(super) fn touch(&self) {
        *self.last_used.lock().unwrap_or_else(PoisonError::into_inner) = Instant::now();
    }

    /// How long ago this server finished starting.
    pub fn uptime(&self) -> Duration {
        self.started.elapsed()
    }

    /// How long since anything last used this server.
    pub fn idle_for(&self) -> Duration {
        self.last_used.lock().unwrap_or_else(PoisonError::into_inner).elapsed()
    }

    /// Asks the server to shut down and exit, then kills whatever is left of its process.
    pub async fn shutdown(&self) {
        if self.is_alive() {
            let _ = self
                .connection
                .request("shutdown", Value::Null, SHUTDOWN_REQUEST_TIMEOUT)
                .await;
            let _ = self.connection.notify("exit", Value::Null);
            let deadline = Instant::now() + EXIT_WAIT;
            while self.is_alive() && Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        drop(self.process.lock().unwrap_or_else(PoisonError::into_inner).take());
    }

    pub fn encoding(&self) -> PositionEncoding {
        self.encoding
    }

    pub fn is_alive(&self) -> bool {
        !self.connection.is_closed()
    }

    pub fn supports(&self, capability: &str) -> bool {
        self.capabilities
            .get(capability)
            .is_some_and(|value| !matches!(value, Value::Null | Value::Bool(false)))
    }

    pub fn state(&self) -> &ClientState {
        &self.state
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        self.touch();
        self.connection.request(method, params, REQUEST_TIMEOUT).await
    }

    /// What the server is busy with right now, if anything.
    pub fn busy(&self) -> Option<String> {
        self.state.busy(self.started.elapsed() >= STARTUP_GRACE)
    }

    /// Waits until the server is idle or `deadline` passes; on timeout returns what it is busy with.
    pub async fn wait_until_idle(&self, deadline: Instant) -> Result<(), String> {
        loop {
            let notified = self.state.changed.notified();
            let Some(busy) = self.busy() else {
                return Ok(());
            };
            if Instant::now() >= deadline || !self.is_alive() {
                return Err(busy);
            }
            let tick = (Instant::now() + Duration::from_millis(100)).min(deadline);
            tokio::select! {
                () = notified => {}
                () = tokio::time::sleep_until(tick) => {}
            }
        }
    }

    pub async fn is_open(&self, path: &Path) -> bool {
        self.documents.lock().await.open.contains_key(&file_uri(path))
    }

    /// Brings every open document up to date with disk, before a request whose answer spans files;
    /// a document whose file is gone or unreadable is closed.
    pub async fn sync_open_documents_from_disk(&self) -> Result<(), String> {
        let uris: Vec<String> = self.documents.lock().await.open.keys().cloned().collect();
        let mut changes = Vec::new();
        for uri in uris {
            if let Some(change) = self.sync_from_disk(uri.clone()).await? {
                changes.push((uri, change));
            }
        }
        self.did_change_watched_files(&changes)
    }

    /// Brings one open document up to date with disk; says how the file changed, if it did.
    async fn sync_from_disk(&self, uri: String) -> Result<Option<FileChange>, String> {
        let path = uri_path(&uri);
        let text = match &path {
            Some(path) => tokio::fs::read_to_string(path).await.ok(),
            None => None,
        };
        let (Some(path), Some(text)) = (path, text) else {
            self.close_uri(&uri).await?;
            return Ok(Some(FileChange::Deleted));
        };
        let changed = self
            .documents
            .lock()
            .await
            .open
            .get(&uri)
            .is_some_and(|open| open.text != text);
        self.sync(&path, &text, "").await?;
        Ok(changed.then_some(FileChange::Changed))
    }

    /// Tells the server that files changed on disk (`workspace/didChangeWatchedFiles`).
    pub fn did_change_watched_files(&self, changes: &[(String, FileChange)]) -> Result<(), String> {
        if changes.is_empty() {
            return Ok(());
        }
        let changes: Vec<Value> = changes
            .iter()
            .map(|(uri, change)| json!({ "uri": uri, "type": change.code() }))
            .collect();
        self.connection
            .notify("workspace/didChangeWatchedFiles", json!({ "changes": changes }))
    }

    /// Tells the server the document is closed; a document that is not open is left alone.
    pub async fn close_document(&self, path: &Path) -> Result<(), String> {
        self.close_uri(&file_uri(path)).await
    }

    async fn close_uri(&self, uri: &str) -> Result<(), String> {
        if self.documents.lock().await.open.remove(uri).is_none() {
            return Ok(());
        }
        self.connection
            .notify("textDocument/didClose", json!({ "textDocument": { "uri": uri } }))
    }

    /// Sends `text` as the document's full content: `didOpen` the first time, `didChange` after.
    pub async fn sync(&self, path: &Path, text: &str, language_id: &str) -> Result<Synced, String> {
        self.touch();
        let uri = file_uri(path);
        let mut documents = self.documents.lock().await;
        let used = documents.tick();
        let baseline = self.state.seq_for(&uri);
        let (method, params, version) = match documents.open.get_mut(&uri) {
            Some(open) if open.text == text => {
                open.used = used;
                return Ok(Synced {
                    version: open.version,
                    baseline: open.baseline,
                });
            }
            Some(open) => {
                open.version += 1;
                open.text = text.to_string();
                open.baseline = baseline;
                open.used = used;
                let params = json!({
                    "textDocument": { "uri": uri, "version": open.version },
                    "contentChanges": [{ "text": text }],
                });
                ("textDocument/didChange", params, open.version)
            }
            None => {
                let open = OpenDocument {
                    version: 1,
                    text: text.to_string(),
                    baseline,
                    used,
                };
                documents.open.insert(uri.clone(), open);
                let params = json!({
                    "textDocument": { "uri": uri, "languageId": language_id, "version": 1, "text": text },
                });
                ("textDocument/didOpen", params, 1)
            }
        };
        self.connection.notify(method, params)?;
        if let Some(stale) = documents.evict_candidate(&uri) {
            documents.open.remove(&stale);
            self.connection
                .notify("textDocument/didClose", json!({ "textDocument": { "uri": stale } }))?;
        }
        Ok(Synced { version, baseline })
    }

    pub async fn did_save(&self, path: &Path, text: &str) -> Result<(), String> {
        self.touch();
        let params = json!({ "textDocument": { "uri": file_uri(path) }, "text": text });
        self.connection.notify("textDocument/didSave", params)
    }

    fn pulls_diagnostics(&self) -> bool {
        self.supports("diagnosticProvider") || self.state.pull_registered.load(Ordering::SeqCst)
    }

    async fn pull(&self, uri: &str, deadline: Instant) -> Option<Vec<Diagnostic>> {
        let wait = deadline.saturating_duration_since(Instant::now());
        let params = json!({ "textDocument": { "uri": uri } });
        let report = self
            .connection
            .request("textDocument/diagnostic", params, wait)
            .await
            .ok()?;
        let items = report.get("items")?.as_array()?;
        Some(items.iter().filter_map(Diagnostic::from_lsp).collect())
    }

    /// Diagnostics for `path` as of `synced`, within `wait`; also waits out work the change started
    /// (such as a check on save), which publishes more diagnostics for the file.
    pub async fn diagnostics(&self, path: &Path, synced: Synced, wait: Duration) -> DiagnosticsOutcome {
        self.touch();
        let deadline = Instant::now() + wait;
        let uri = file_uri(path);
        let pulled = match self.pulls_diagnostics() {
            true => self.pull(&uri, deadline).await,
            false => None,
        };
        let first = match pulled.clone() {
            Some(items) => Some(items),
            None => self.wait_for_push(&uri, synced, deadline).await,
        };
        if self.state.reports_progress() {
            tokio::time::sleep_until((Instant::now() + FOLLOW_UP_GRACE).min(deadline)).await;
        }
        let idle = self.wait_until_idle(deadline).await;
        let latest = match pulled {
            Some(items) => Some(items),
            None => self.state.latest_after(&uri, synced.baseline, synced.version).or(first),
        };
        match (latest, idle) {
            (Some(items), Ok(())) => DiagnosticsOutcome::Ready(items),
            (Some(items), Err(busy)) => DiagnosticsOutcome::Partial(items, busy),
            (None, idle) => DiagnosticsOutcome::Pending(idle.err()),
        }
    }

    async fn wait_for_push(&self, uri: &str, synced: Synced, deadline: Instant) -> Option<Vec<Diagnostic>> {
        loop {
            let notified = self.state.changed.notified();
            let wake = match self.check_published(uri, synced) {
                PushState::Done(items) => return Some(items),
                PushState::SettlesAt(at) => at.min(deadline),
                PushState::Waiting => deadline,
            };
            if Instant::now() >= deadline || !self.is_alive() {
                return None;
            }
            tokio::select! {
                () = notified => {}
                () = tokio::time::sleep_until(wake) => {}
            }
        }
    }

    fn check_published(&self, uri: &str, synced: Synced) -> PushState {
        let published = self.state.published();
        let Some(latest) = published.get(uri).filter(|p| p.seq > synced.baseline) else {
            return PushState::Waiting;
        };
        match latest.version {
            Some(version) if version == synced.version => PushState::Done(latest.items.clone()),
            Some(version) if version < synced.version => PushState::Waiting,
            _ if latest.received.elapsed() >= DIAGNOSTICS_SETTLE => PushState::Done(latest.items.clone()),
            _ => PushState::SettlesAt(latest.received + DIAGNOSTICS_SETTLE),
        }
    }
}

enum PushState {
    Done(Vec<Diagnostic>),
    SettlesAt(Instant),
    Waiting,
}

#[cfg(test)]
mod tests;
