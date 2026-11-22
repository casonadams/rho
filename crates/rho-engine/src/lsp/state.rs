//! What a language server tells rho on its own: diagnostics, progress, and requests for
//! configuration.

use super::rpc::ServerMessages;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use tokio::sync::Notify;
use tokio::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// 1-based line and column; the column is in the server's position encoding units.
    pub line: u32,
    pub column: u32,
    pub severity: u8,
    pub message: String,
    pub source: Option<String>,
    pub code: Option<String>,
    /// The diagnostic as the server sent it, to hand back as code action context.
    pub raw: Value,
}

impl Diagnostic {
    pub fn from_lsp(value: &Value) -> Option<Self> {
        let start = value.get("range")?.get("start")?;
        let code = value.get("code").and_then(|code| match code {
            Value::String(text) => Some(text.clone()),
            Value::Number(number) => Some(number.to_string()),
            _ => None,
        });
        Some(Self {
            line: start.get("line")?.as_u64()? as u32 + 1,
            column: start.get("character")?.as_u64()? as u32 + 1,
            severity: value.get("severity").and_then(Value::as_u64).unwrap_or(1) as u8,
            message: value.get("message")?.as_str()?.to_string(),
            source: value.get("source").and_then(Value::as_str).map(str::to_string),
            code,
            raw: value.clone(),
        })
    }
}

pub struct Published {
    pub version: Option<i64>,
    pub items: Vec<Diagnostic>,
    pub seq: u64,
    pub received: Instant,
}

#[derive(Default)]
struct Activity {
    /// Work in progress by `$/progress` token, with its title.
    progress: HashMap<String, String>,
    progress_seen: bool,
    /// rust-analyzer's `experimental/serverStatus`: false while it loads the project.
    quiescent: Option<bool>,
}

pub struct ClientState {
    settings: Value,
    pub(super) workspace_folder: Value,
    published: Mutex<HashMap<String, Published>>,
    activity: Mutex<Activity>,
    seq: AtomicU64,
    pub(super) changed: Notify,
    pub(super) pull_registered: AtomicBool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn progress_token(params: &Value) -> String {
    match params.get("token") {
        Some(Value::String(token)) => token.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

/// The URI rho itself would build for the same file, so lookups match however the server encodes it.
pub fn normalize_uri(uri: &str) -> String {
    super::client::uri_path(uri).map_or_else(|| uri.to_string(), |path| super::client::file_uri(&path))
}

impl ClientState {
    pub fn new(settings: Option<Value>, root: &Path) -> Self {
        let uri = super::client::file_uri(root);
        let name = root.file_name().and_then(|n| n.to_str()).unwrap_or("workspace");
        Self {
            settings: settings.unwrap_or(Value::Null),
            workspace_folder: json!({ "uri": uri, "name": name }),
            published: Mutex::new(HashMap::new()),
            activity: Mutex::new(Activity::default()),
            seq: AtomicU64::new(0),
            changed: Notify::new(),
            pull_registered: AtomicBool::new(false),
        }
    }

    pub(super) fn published(&self) -> MutexGuard<'_, HashMap<String, Published>> {
        lock(&self.published)
    }

    pub(super) fn seq_for(&self, uri: &str) -> u64 {
        self.published().get(uri).map_or(0, |p| p.seq)
    }

    /// The diagnostics last published for `uri`.
    pub fn published_for(&self, uri: &str) -> Vec<Diagnostic> {
        self.published()
            .get(&normalize_uri(uri))
            .map_or_else(Vec::new, |p| p.items.clone())
    }

    /// The latest diagnostics for `uri` published after sequence number `after`, unless they are
    /// for a document version older than `version`.
    pub(super) fn latest_after(&self, uri: &str, after: u64, version: i64) -> Option<Vec<Diagnostic>> {
        self.published()
            .get(uri)
            .filter(|p| p.seq > after && p.version.is_none_or(|v| v >= version))
            .map(|p| p.items.clone())
    }

    fn record(&self, params: &Value) {
        let Some(uri) = params.get("uri").and_then(Value::as_str) else {
            return;
        };
        let items = params
            .get("diagnostics")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(Diagnostic::from_lsp).collect())
            .unwrap_or_default();
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let published = Published {
            version: params.get("version").and_then(Value::as_i64),
            items,
            seq,
            received: Instant::now(),
        };
        self.published().insert(normalize_uri(uri), published);
    }

    fn progress(&self, params: &Value) {
        let token = progress_token(params);
        let value = params.get("value").unwrap_or(&Value::Null);
        let mut activity = lock(&self.activity);
        match value.get("kind").and_then(Value::as_str) {
            Some("begin") => {
                let title = value.get("title").and_then(Value::as_str).unwrap_or("working");
                activity.progress.insert(token, title.to_string());
                activity.progress_seen = true;
            }
            Some("end") => {
                activity.progress.remove(&token);
            }
            _ => {}
        }
    }

    fn server_status(&self, params: &Value) {
        lock(&self.activity).quiescent = params.get("quiescent").and_then(Value::as_bool);
    }

    /// What the server is busy with, or `None` when it is idle. A server that has reported
    /// neither progress nor status counts as idle once `grace_over`.
    pub fn busy(&self, grace_over: bool) -> Option<String> {
        let activity = lock(&self.activity);
        if activity.quiescent == Some(false) {
            return Some("loading the project".into());
        }
        if !activity.progress.is_empty() {
            let mut titles: Vec<&str> = activity.progress.values().map(String::as_str).collect();
            titles.sort_unstable();
            titles.dedup();
            return Some(titles.join(", "));
        }
        let reported = activity.progress_seen || activity.quiescent.is_some();
        (!reported && !grace_over).then(|| "starting".to_string())
    }

    /// Whether the server has ever reported progress or status, so it would report follow-up work.
    pub fn reports_progress(&self) -> bool {
        let activity = lock(&self.activity);
        activity.progress_seen || activity.quiescent.is_some()
    }

    fn configuration(&self, params: &Value) -> Value {
        let items = params
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Value::Array(
            items
                .iter()
                .map(|item| {
                    let section = item.get("section").and_then(Value::as_str).unwrap_or("");
                    section_value(&self.settings, section)
                })
                .collect(),
        )
    }

    fn register(&self, params: &Value) {
        let registrations = params.get("registrations").and_then(Value::as_array);
        if registrations
            .into_iter()
            .flatten()
            .any(|r| r.get("method").and_then(Value::as_str) == Some("textDocument/diagnostic"))
        {
            self.pull_registered.store(true, Ordering::SeqCst);
        }
    }

    /// Every file's latest published diagnostics, by URI.
    pub fn all_published(&self) -> Vec<(String, Vec<Diagnostic>)> {
        let mut all: Vec<_> = self
            .published()
            .iter()
            .filter(|(_, p)| !p.items.is_empty())
            .map(|(uri, p)| (uri.clone(), p.items.clone()))
            .collect();
        all.sort_by(|a, b| a.0.cmp(&b.0));
        all
    }
}

fn section_value(settings: &Value, section: &str) -> Value {
    if section.is_empty() {
        return settings.clone();
    }
    let direct = settings.get(section);
    let nested = section.split('.').try_fold(settings, |value, key| value.get(key));
    direct.or(nested).cloned().unwrap_or(Value::Null)
}

impl ServerMessages for ClientState {
    fn notification(&self, method: &str, params: Value) {
        match method {
            "textDocument/publishDiagnostics" => self.record(&params),
            "$/progress" => self.progress(&params),
            "experimental/serverStatus" => self.server_status(&params),
            _ => return,
        }
        self.changed.notify_waiters();
    }

    fn closed(&self) {
        self.changed.notify_waiters();
    }

    fn request(&self, method: &str, params: &Value) -> Option<Value> {
        match method {
            "workspace/configuration" => Some(self.configuration(params)),
            "workspace/workspaceFolders" => Some(json!([self.workspace_folder])),
            "client/registerCapability" => {
                self.register(params);
                Some(Value::Null)
            }
            "client/unregisterCapability" | "window/workDoneProgress/create" => Some(Value::Null),
            "workspace/applyEdit" => Some(json!({ "applied": false, "failureReason": "rho applies edits itself" })),
            "window/showDocument" => Some(json!({ "success": false })),
            "window/showMessageRequest" => Some(Value::Null),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(state: &ClientState, token: &str, kind: &str, title: &str) {
        state.notification(
            "$/progress",
            json!({ "token": token, "value": { "kind": kind, "title": title } }),
        );
    }

    #[test]
    fn busy_tracks_progress_and_server_status() {
        let state = ClientState::new(None, Path::new("/ws"));
        assert_eq!(state.busy(false).as_deref(), Some("starting"));
        assert_eq!(state.busy(true), None, "silent servers are idle after the grace period");

        progress(&state, "flycheck", "begin", "cargo check");
        progress(&state, "index", "begin", "Indexing");
        assert_eq!(state.busy(true).as_deref(), Some("Indexing, cargo check"));
        progress(&state, "index", "end", "");
        progress(&state, "flycheck", "end", "");
        assert_eq!(
            state.busy(false),
            None,
            "a server that reported progress needs no grace"
        );

        state.notification(
            "experimental/serverStatus",
            json!({ "quiescent": false, "health": "ok" }),
        );
        assert_eq!(state.busy(true).as_deref(), Some("loading the project"));
        state.notification(
            "experimental/serverStatus",
            json!({ "quiescent": true, "health": "ok" }),
        );
        assert_eq!(state.busy(true), None);
    }

    #[test]
    fn configuration_requests_read_sections_from_settings() {
        let settings = json!({ "rust-analyzer": { "check": { "command": "clippy" } }, "a.b": 1 });
        let state = ClientState::new(Some(settings), Path::new("/ws"));
        let answer = state.request(
            "workspace/configuration",
            &json!({ "items": [{ "section": "rust-analyzer.check" }, { "section": "a.b" }, { "section": "none" }] }),
        );
        assert_eq!(answer, Some(json!([{ "command": "clippy" }, 1, null])));
        assert_eq!(state.request("workspace/unknownThing", &Value::Null), None);
    }

    #[test]
    fn diagnostics_match_however_the_server_encodes_the_uri() {
        let state = ClientState::new(None, Path::new("/ws"));
        let diagnostic = json!({ "range": { "start": { "line": 0, "character": 0 } }, "message": "bad" });
        state.notification(
            "textDocument/publishDiagnostics",
            json!({ "uri": "file:///ws/node_modules/%40scope/a.ts", "diagnostics": [diagnostic] }),
        );
        let ours = super::super::client::file_uri(Path::new("/ws/node_modules/@scope/a.ts"));
        assert_eq!(state.latest_after(&ours, 0, 1).map(|items| items.len()), Some(1));
    }
}
