//! An in-process language server for tests. Diagnostics: a line containing `ERR` is an error and
//! `WARN` a warning. Symbols: `struct Name` and `fn name` lines; a block ends at a line `}`.
//! Code actions: `ERR` offers a quick fix removing it, `WARN` a fix that needs `codeAction/resolve`,
//! `CMD` an action that only has a command, `SECRET` an edit to `.env.rs`. Formatting trims
//! trailing spaces.

use super::config::ServerConfig;
use super::manager::{Launched, Launcher, LspManager};
use super::rpc::{BoxedWriter, read_message, write_message};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::BufReader;

mod queries;

#[derive(Clone, Copy, Default)]
pub struct FakeOptions {
    /// Answer `textDocument/diagnostic` instead of publishing.
    pub pull: bool,
    /// Publish without a document version.
    pub unversioned: bool,
    /// Never publish diagnostics.
    pub silent: bool,
    /// Report an `Indexing` progress that never ends.
    pub indexing: bool,
    /// Fail to launch.
    pub fail: bool,
    /// Exit when a document changes.
    pub exit_on_change: bool,
    /// Offer UTF-8 positions.
    pub utf8: bool,
    /// Register pull diagnostics dynamically instead of advertising them.
    pub dynamic_pull: bool,
    /// Advertise pull diagnostics but answer every pull with an error, and publish instead.
    pub pull_fails: bool,
    /// Publish diagnostics for the version before the one just received.
    pub stale_version: bool,
    /// Ask the client for its `fake.opt` configuration after startup.
    pub ask_configuration: bool,
    /// Never answer a request other than `initialize`.
    pub unresponsive: bool,
    /// Never answer `initialize`.
    pub hang_init: bool,
    /// Report neither progress nor status after startup.
    pub no_status: bool,
    /// Milliseconds the server takes to publish diagnostics after a document opens or changes.
    pub publish_delay_ms: u64,
    /// Like rust-analyzer: publish nothing on open, then after a save start a `cargo check` that
    /// reports the real diagnostics a moment later.
    pub check_on_save: bool,
    /// Do not advertise `textDocumentSync.save`.
    pub no_save: bool,
    /// Ask for the text in `didSave`.
    pub save_text: bool,
}

#[derive(Default)]
pub struct FakeLauncher {
    pub options: FakeOptions,
    pub launches: AtomicUsize,
    pub received: Arc<Mutex<Vec<String>>>,
    /// The results of the requests the server sent the client.
    pub responses: Arc<Mutex<Vec<Value>>>,
    /// The method and params of each `initialize`, `workspace/didChangeWatchedFiles`, `textDocument/didSave`,
    /// `textDocument/codeAction` and `textDocument/formatting` the server received.
    pub params: Arc<Mutex<Vec<(String, Value)>>>,
}

/// Polls `condition` every 10ms of (virtual) time and fails the test if it stays false for 30 seconds.
pub async fn until(what: &str, mut condition: impl FnMut() -> bool) {
    let waiting = async {
        while !condition() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(30), waiting)
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
}

/// The fake server's configuration: `.rs` files under a `Cargo.toml`.
pub fn fake_server(name: &str, linter: bool) -> ServerConfig {
    let command = std::env::current_exe().unwrap();
    let mut server: ServerConfig = serde_json::from_value(json!({
        "command": command,
        "extensions": ["rs"],
        "rootMarkers": ["Cargo.toml"],
        "linter": linter,
    }))
    .unwrap();
    server.name = name.into();
    server
}

/// A trusting manager whose only server is the fake one.
pub fn fake_manager(workspace: &Path, options: FakeOptions) -> (Arc<LspManager>, Arc<FakeLauncher>) {
    fake_manager_with(workspace, options, vec![fake_server("fake", false)])
}

pub fn fake_manager_with(
    workspace: &Path,
    options: FakeOptions,
    servers: Vec<ServerConfig>,
) -> (Arc<LspManager>, Arc<FakeLauncher>) {
    let launcher = Arc::new(FakeLauncher {
        options,
        ..FakeLauncher::default()
    });
    let manager = LspManager::new(
        workspace,
        Ok(crate::lsp::config::ServerSet {
            servers,
            ..Default::default()
        }),
        None,
        Arc::clone(&launcher) as Arc<dyn Launcher>,
    )
    .with_trust(true);
    (Arc::new(manager), launcher)
}

impl Launcher for FakeLauncher {
    fn launch(&self, _: &ServerConfig, _: &Path, _: &Path) -> Result<Launched, String> {
        self.launches.fetch_add(1, Ordering::SeqCst);
        if self.options.fail {
            return Err("fake: command not found".into());
        }
        let (client_side, server_side) = tokio::io::duplex(1 << 20);
        let (client_read, client_write) = tokio::io::split(client_side);
        let (server_read, server_write) = tokio::io::split(server_side);
        let server = FakeServer {
            options: self.options,
            docs: HashMap::new(),
            received: Arc::clone(&self.received),
            responses: Arc::clone(&self.responses),
            params: Arc::clone(&self.params),
        };
        tokio::spawn(server.run(Box::new(server_read), Box::new(server_write)));
        Ok(Launched {
            reader: Box::new(client_read),
            writer: Box::new(client_write),
            process: None,
            stderr: Default::default(),
        })
    }
}

struct FakeServer {
    options: FakeOptions,
    docs: HashMap<String, (i64, String)>,
    received: Arc<Mutex<Vec<String>>>,
    responses: Arc<Mutex<Vec<Value>>>,
    params: Arc<Mutex<Vec<(String, Value)>>>,
}

fn word_at(text: &str, line: u64, character: u64) -> String {
    let line = text.lines().nth(line as usize).unwrap_or("");
    let at = (character as usize).min(line.len());
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let start = line[..at].rfind(|c: char| !is_word(c)).map_or(0, |i| i + 1);
    let end = line[at..].find(|c: char| !is_word(c)).map_or(line.len(), |i| at + i);
    line[start..end].to_string()
}

fn range(line: usize, start: usize, end_line: usize, end: usize) -> Value {
    json!({ "start": { "line": line, "character": start }, "end": { "line": end_line, "character": end } })
}

fn marked_diagnostics(text: &str) -> Vec<Value> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        for (marker, severity) in [("ERR", 1), ("WARN", 2)] {
            if let Some(col) = line.find(marker) {
                found.push(json!({
                    "range": range(index, col, index, col + marker.len()),
                    "severity": severity,
                    "source": "fake",
                    "message": format!("{marker} found\nsecond line"),
                }));
            }
        }
    }
    found
}

fn block_end(lines: &[&str], start: usize, closing: &str) -> usize {
    lines[start..]
        .iter()
        .position(|line| line.trim_end() == closing)
        .map_or(start, |offset| start + offset)
}

fn symbols(text: &str) -> Vec<Value> {
    let lines: Vec<&str> = text.lines().collect();
    let mut top = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let name = |prefix: &str| line.trim_start().strip_prefix(prefix).map(|rest| word_at(rest, 0, 0));
        if let Some(name) = name("struct ").filter(|_| !line.starts_with(' ')) {
            let end = block_end(&lines, index, "}");
            top.push(json!({ "name": name, "kind": 23, "range": range(index, 0, end, 1),
                "selectionRange": range(index, 7, index, 7 + name.len()), "children": [] }));
        } else if let Some(name) = name("fn ") {
            let nested = line.starts_with(' ');
            let end = block_end(&lines, index, if nested { "    }" } else { "}" });
            let symbol = json!({ "name": name, "kind": 12, "detail": "fn()", "range": range(index, 0, end, 1),
                "selectionRange": range(index, 0, index, 1) });
            match top.last_mut().filter(|_| nested) {
                Some(parent) => parent["children"].as_array_mut().unwrap().push(symbol),
                None => top.push(symbol),
            }
        }
    }
    top
}

impl FakeServer {
    async fn run(mut self, reader: super::rpc::BoxedReader, mut writer: BoxedWriter) {
        let mut reader = BufReader::new(reader);
        while let Ok(Some(message)) = read_message(&mut reader).await {
            let method = message.get("method").and_then(Value::as_str).unwrap_or("").to_string();
            if method.is_empty() {
                self.responses
                    .lock()
                    .unwrap()
                    .push(message.get("result").cloned().unwrap_or(Value::Null));
                continue;
            }
            self.received.lock().unwrap().push(method.clone());
            if matches!(
                method.as_str(),
                "initialize"
                    | "workspace/didChangeWatchedFiles"
                    | "textDocument/didSave"
                    | "textDocument/codeAction"
                    | "textDocument/formatting"
            ) {
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                self.params.lock().unwrap().push((method.clone(), params));
            }
            if method == "textDocument/didChange" && self.options.exit_on_change {
                return;
            }
            if let Some(id) = message.get("id")
                && let Some(error) = self.refusal(&method)
            {
                let _ = write_message(&mut writer, &json!({ "jsonrpc": "2.0", "id": id, "error": error })).await;
                continue;
            }
            if message.get("id").is_some() && self.ignores(&method) {
                continue;
            }
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            let outgoing = self.handle(&method, &params);
            if let Some(id) = message.get("id") {
                let result = outgoing.0;
                let _ = write_message(&mut writer, &json!({ "jsonrpc": "2.0", "id": id, "result": result })).await;
            }
            for notification in outgoing.1 {
                if self.options.publish_delay_ms > 0 && method.starts_with("textDocument/did") {
                    tokio::time::sleep(std::time::Duration::from_millis(self.options.publish_delay_ms)).await;
                }
                let _ = write_message(&mut writer, &notification).await;
            }
            if method == "textDocument/didSave" && self.options.check_on_save {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or("").to_string();
                self.run_check(&uri, &mut writer).await;
            }
            if method == "exit" {
                return;
            }
        }
    }

    async fn run_check(&self, uri: &str, writer: &mut super::rpc::BoxedWriter) {
        let pause = |ms| tokio::time::sleep(std::time::Duration::from_millis(ms));
        let progress = |kind: &str| {
            json!({ "jsonrpc": "2.0", "method": "$/progress", "params": {
                "token": "flycheck", "value": { "kind": kind, "title": "cargo check" } } })
        };
        pause(300).await;
        let _ = write_message(writer, &progress("begin")).await;
        pause(1000).await;
        let (version, text) = self.docs.get(uri).cloned().unwrap_or_default();
        let publish = json!({ "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {
            "uri": uri, "version": version, "diagnostics": marked_diagnostics(&text) } });
        let _ = write_message(writer, &publish).await;
        let _ = write_message(writer, &progress("end")).await;
    }

    /// An error to answer `method` with instead of handling it.
    fn refusal(&self, method: &str) -> Option<Value> {
        (method == "textDocument/diagnostic" && self.options.pull_fails)
            .then(|| json!({ "code": -32803, "message": "pull refused" }))
    }

    /// Whether the request is left unanswered.
    fn ignores(&self, method: &str) -> bool {
        match method {
            "initialize" => self.options.hang_init,
            _ => self.options.unresponsive,
        }
    }

    fn handle(&mut self, method: &str, params: &Value) -> (Value, Vec<Value>) {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("").to_string();
        match method {
            "initialize" => (self.capabilities(), Vec::new()),
            "initialized" => (Value::Null, self.startup_notifications()),
            "textDocument/didOpen" => {
                let doc = &params["textDocument"];
                self.store(
                    &uri,
                    doc["version"].as_i64().unwrap_or(0),
                    doc["text"].as_str().unwrap_or(""),
                )
            }
            "textDocument/didChange" => {
                let text = params["contentChanges"][0]["text"].as_str().unwrap_or("");
                self.store(&uri, params["textDocument"]["version"].as_i64().unwrap_or(0), text)
            }
            "textDocument/diagnostic" => (
                json!({ "kind": "full", "items": marked_diagnostics(self.text(&uri)) }),
                Vec::new(),
            ),
            "textDocument/documentSymbol" => (Value::Array(symbols(self.text(&uri))), Vec::new()),
            "textDocument/hover" => (
                json!({ "contents": { "kind": "markdown", "value": format!("`{}`: hover", self.word(params)) } }),
                Vec::new(),
            ),
            "textDocument/definition" => (self.definition(params), Vec::new()),
            "textDocument/references" => (Value::Array(self.occurrences(&self.word(params))), Vec::new()),
            "textDocument/rename" => (self.rename(params), Vec::new()),
            "textDocument/codeAction" => (self.code_actions(params), Vec::new()),
            "codeAction/resolve" => (self.resolve_action(params), Vec::new()),
            "textDocument/formatting" => (self.formatting(&uri), Vec::new()),
            "workspace/symbol" => (
                self.workspace_symbols(params["query"].as_str().unwrap_or("")),
                Vec::new(),
            ),
            _ => (Value::Null, Vec::new()),
        }
    }

    fn startup_notifications(&self) -> Vec<Value> {
        let notification =
            |method: &str, params: Value| json!({ "jsonrpc": "2.0", "method": method, "params": params });
        let request = |id: i64, method: &str, params: Value| json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let mut messages = Vec::new();
        if self.options.dynamic_pull {
            let registration = json!({ "registrations": [{ "id": "1", "method": "textDocument/diagnostic" }] });
            messages.push(request(9001, "client/registerCapability", registration));
        }
        if self.options.ask_configuration {
            let items = json!({ "items": [{ "section": "fake.opt" }] });
            messages.push(request(9002, "workspace/configuration", items));
        }
        if self.options.no_status {
            return messages;
        }
        messages.push(if self.options.indexing {
            notification(
                "$/progress",
                json!({ "token": "index", "value": { "kind": "begin", "title": "Indexing" } }),
            )
        } else {
            notification(
                "experimental/serverStatus",
                json!({ "quiescent": true, "health": "ok" }),
            )
        });
        messages
    }

    fn capabilities(&self) -> Value {
        let mut sync = json!({ "openClose": true, "change": 1 });
        if !self.options.no_save {
            sync["save"] = json!({ "includeText": self.options.save_text });
        }
        let mut caps = json!({
            "textDocumentSync": sync, "hoverProvider": true, "definitionProvider": true,
            "referencesProvider": true, "documentSymbolProvider": true, "renameProvider": true,
            "workspaceSymbolProvider": true, "codeActionProvider": { "resolveProvider": true },
            "documentFormattingProvider": true,
        });
        if self.options.pull || self.options.pull_fails {
            caps["diagnosticProvider"] = json!({ "interFileDependencies": false, "workspaceDiagnostics": false });
        }
        if self.options.utf8 {
            caps["positionEncoding"] = json!("utf-8");
        }
        json!({ "capabilities": caps })
    }

    fn store(&mut self, uri: &str, version: i64, text: &str) -> (Value, Vec<Value>) {
        self.docs.insert(uri.to_string(), (version, text.to_string()));
        let pulled = (self.options.pull && !self.options.pull_fails) || self.options.dynamic_pull;
        if pulled || self.options.silent {
            return (Value::Null, Vec::new());
        }
        let mut params = json!({ "uri": uri, "diagnostics": marked_diagnostics(text) });
        if self.options.check_on_save {
            params["diagnostics"] = json!([]);
        }
        if !self.options.unversioned {
            let shown = if self.options.stale_version {
                version - 1
            } else {
                version
            };
            params["version"] = json!(shown);
        }
        let publish = json!({ "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": params });
        (Value::Null, vec![publish])
    }

    fn text(&self, uri: &str) -> &str {
        self.docs.get(uri).map_or("", |(_, text)| text.as_str())
    }

    fn word(&self, params: &Value) -> String {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
        let position = &params["position"];
        word_at(
            self.text(uri),
            position["line"].as_u64().unwrap_or(0),
            position["character"].as_u64().unwrap_or(0),
        )
    }
}
