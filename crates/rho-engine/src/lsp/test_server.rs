//! An in-process language server for tests. Diagnostics: a line containing `ERR` is an error and
//! `WARN` a warning. Symbols: `struct Name` and `fn name` lines; a block ends at a line `}`.

use super::config::ServerConfig;
use super::manager::{Launched, Launcher, LspManager};
use super::rpc::{BoxedWriter, read_message, write_message};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::BufReader;

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
}

#[derive(Default)]
pub struct FakeLauncher {
    pub options: FakeOptions,
    pub launches: AtomicUsize,
    pub received: Arc<Mutex<Vec<String>>>,
}

/// A manager whose only server is the fake one, for `.rs` files under a `Cargo.toml`.
pub fn fake_manager(workspace: &Path, options: FakeOptions) -> (Arc<LspManager>, Arc<FakeLauncher>) {
    let launcher = Arc::new(FakeLauncher {
        options,
        ..FakeLauncher::default()
    });
    let command = std::env::current_exe().unwrap();
    let mut server: ServerConfig = serde_json::from_value(json!({
        "command": command,
        "extensions": ["rs"],
        "rootMarkers": ["Cargo.toml"],
    }))
    .unwrap();
    server.name = "fake".into();
    let manager = LspManager::new(
        workspace,
        Ok(crate::lsp::config::ServerSet {
            servers: vec![server],
            notice: None,
        }),
        None,
        Arc::clone(&launcher) as Arc<dyn Launcher>,
    );
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
            self.received.lock().unwrap().push(method.clone());
            if method == "textDocument/didChange" && self.options.exit_on_change {
                return;
            }
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            let outgoing = self.handle(&method, &params);
            if let Some(id) = message.get("id") {
                let result = outgoing.0;
                let _ = write_message(&mut writer, &json!({ "jsonrpc": "2.0", "id": id, "result": result })).await;
            }
            for notification in outgoing.1 {
                let _ = write_message(&mut writer, &notification).await;
            }
            if method == "exit" {
                return;
            }
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
        if self.options.indexing {
            vec![notification(
                "$/progress",
                json!({ "token": "index", "value": { "kind": "begin", "title": "Indexing" } }),
            )]
        } else {
            vec![notification(
                "experimental/serverStatus",
                json!({ "quiescent": true, "health": "ok" }),
            )]
        }
    }

    fn capabilities(&self) -> Value {
        let mut caps = json!({
            "textDocumentSync": 1, "hoverProvider": true, "definitionProvider": true,
            "referencesProvider": true, "documentSymbolProvider": true, "renameProvider": true,
            "workspaceSymbolProvider": true,
        });
        if self.options.pull {
            caps["diagnosticProvider"] = json!({ "interFileDependencies": false, "workspaceDiagnostics": false });
        }
        json!({ "capabilities": caps })
    }

    fn store(&mut self, uri: &str, version: i64, text: &str) -> (Value, Vec<Value>) {
        self.docs.insert(uri.to_string(), (version, text.to_string()));
        if self.options.pull || self.options.silent {
            return (Value::Null, Vec::new());
        }
        let mut params = json!({ "uri": uri, "diagnostics": marked_diagnostics(text) });
        if !self.options.unversioned {
            params["version"] = json!(version);
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

    fn occurrences(&self, word: &str) -> Vec<Value> {
        let mut found = Vec::new();
        let mut uris: Vec<&String> = self.docs.keys().collect();
        uris.sort();
        for uri in uris {
            for (index, line) in self.text(uri).lines().enumerate() {
                for (col, _) in line.match_indices(word) {
                    if word_at(line, 0, col as u64) == word {
                        found.push(json!({ "uri": uri, "range": range(index, col, index, col + word.len()) }));
                    }
                }
            }
        }
        found
    }

    fn definition(&self, params: &Value) -> Value {
        let word = self.word(params);
        let definitions: Vec<Value> = self
            .occurrences(&word)
            .into_iter()
            .filter(|location| {
                let uri = location["uri"].as_str().unwrap_or("");
                let line = location["range"]["start"]["line"].as_u64().unwrap_or(0) as usize;
                let text = self.text(uri).lines().nth(line).unwrap_or("");
                text.contains(&format!("fn {word}")) || text.contains(&format!("struct {word}"))
            })
            .collect();
        Value::Array(definitions)
    }

    fn rename(&self, params: &Value) -> Value {
        let new_name = params["newName"].as_str().unwrap_or("");
        let mut changes = serde_json::Map::new();
        for location in self.occurrences(&self.word(params)) {
            let uri = location["uri"].as_str().unwrap_or("").to_string();
            let edit = json!({ "range": location["range"], "newText": new_name });
            changes
                .entry(uri)
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .unwrap()
                .push(edit);
        }
        json!({ "changes": changes })
    }

    fn workspace_symbols(&self, query: &str) -> Value {
        let found: Vec<Value> = self
            .occurrences(query)
            .into_iter()
            .map(|location| json!({ "name": query, "kind": 12, "location": location }))
            .collect();
        Value::Array(found)
    }
}
