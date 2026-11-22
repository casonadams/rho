use crate::lsp::client::file_uri;
use crate::lsp::position::{character_offset, locate_symbol};
use crate::lsp::{LspClient, LspManager, render};
use crate::tools::engine_tool::TypedTool;
use crate::tools::types::{ToolResult, resolve_tool_path};
use crate::tools::undo::EditHistory;
pub use rho_harness_core::args::{LspAction, LspArgs};
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::{Workspace, clean_path_arg};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub const DIAGNOSTICS_WAIT: Duration = Duration::from_secs(10);
/// How long a query waits for a server that is still loading the project.
pub const READY_WAIT: Duration = Duration::from_secs(30);

pub struct LspTool {
    base_dir: PathBuf,
    manager: Arc<LspManager>,
    history: Option<EditHistory>,
}

/// A file synced to its server, ready for position queries.
struct OpenFile {
    client: Arc<LspClient>,
    path: PathBuf,
    text: String,
    /// What the server was still busy with when the query was sent.
    busy: Option<String>,
}

impl OpenFile {
    /// `result`, with a hint to retry when it is empty because the server was not ready.
    fn answer(&self, result: String, empty: bool) -> String {
        match (&self.busy, empty) {
            (Some(busy), true) => format!(
                "{result}\n[{} was still busy ({busy}); retry shortly]",
                self.client.name
            ),
            _ => result,
        }
    }

    fn text_document(&self) -> Value {
        json!({ "uri": file_uri(&self.path) })
    }

    fn position(&self, line: usize, symbol: Option<&str>) -> Result<Value, String> {
        let total = self.text.lines().count();
        let text = self
            .text
            .lines()
            .nth(line.wrapping_sub(1))
            .ok_or_else(|| format!("line {line} is outside the file (1-{total})"))?;
        let byte = locate_symbol(text, symbol)?;
        let character = character_offset(text, byte, self.client.encoding());
        Ok(json!({ "line": line - 1, "character": character }))
    }
}

fn location_method(action: LspAction) -> Option<(&'static str, &'static str, &'static str)> {
    match action {
        LspAction::Definition => Some(("textDocument/definition", "definitionProvider", "definition")),
        LspAction::References => Some(("textDocument/references", "referencesProvider", "reference")),
        LspAction::Implementation => Some((
            "textDocument/implementation",
            "implementationProvider",
            "implementation",
        )),
        LspAction::TypeDefinition => Some((
            "textDocument/typeDefinition",
            "typeDefinitionProvider",
            "type definition",
        )),
        _ => None,
    }
}

impl LspTool {
    pub fn new(base_dir: impl AsRef<Path>, manager: Arc<LspManager>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
            manager,
            history: None,
        }
    }

    pub fn with_history(mut self, history: Option<EditHistory>) -> Self {
        self.history = history;
        self
    }

    fn resolve(&self, path: Option<&str>) -> Result<PathBuf, String> {
        let path = path.ok_or("this action needs path")?;
        resolve_tool_path(&Workspace::new(&self.base_dir), clean_path_arg(path), "lsp").map_err(|e| e.content)
    }

    async fn open(&self, path: Option<&str>) -> Result<OpenFile, String> {
        let path = self.resolve(path)?;
        let text = tokio::fs::read_to_string(&path)
            .await
            .map_err(|error| format!("reading {}: {error}", path.display()))?;
        let client = self.manager.client_for(&path).await?;
        client.sync_open_documents_from_disk().await?;
        let language = self.language_id(&client, &path);
        client.sync(&path, &text, &language).await?;
        let busy = client
            .wait_until_idle(tokio::time::Instant::now() + READY_WAIT)
            .await
            .err();
        Ok(OpenFile {
            client,
            path,
            text,
            busy,
        })
    }

    fn language_id(&self, client: &LspClient, path: &Path) -> String {
        self.manager
            .servers()
            .iter()
            .find(|server| server.name == client.name)
            .map_or_else(String::new, |server| server.language_id(path))
    }

    async fn run_action(&self, args: LspArgs) -> Result<String, String> {
        if let Some((method, capability, noun)) = location_method(args.action) {
            return self.locations(&args, method, capability, noun).await;
        }
        match args.action {
            LspAction::Diagnostics => self.diagnostics(args.path.as_deref()).await,
            LspAction::Hover => self.hover(&args).await,
            LspAction::Symbols => self.symbols(&args).await,
            LspAction::Rename => self.rename(&args).await,
            _ => Ok(self.status()),
        }
    }

    async fn position_request(&self, args: &LspArgs, method: &str, extra: Value) -> Result<(OpenFile, Value), String> {
        let line = args.line.ok_or("this action needs line (1-based) and usually symbol")?;
        let file = self.open(args.path.as_deref()).await?;
        let mut params = json!({
            "textDocument": file.text_document(),
            "position": file.position(line, args.symbol.as_deref())?,
        });
        if let (Value::Object(params), Value::Object(extra)) = (&mut params, extra) {
            params.extend(extra);
        }
        let response = file.client.request(method, params).await?;
        Ok((file, response))
    }

    async fn locations(&self, args: &LspArgs, method: &str, capability: &str, noun: &str) -> Result<String, String> {
        let extra = match args.action {
            LspAction::References => json!({ "context": { "includeDeclaration": true } }),
            _ => json!({}),
        };
        let (file, response) = self.position_request(args, method, extra).await?;
        if response.is_null() && !file.client.supports(capability) {
            return Err(format!("{} does not support {noun}", file.client.name));
        }
        let found = render::locations(&response);
        let text = render::format_locations(&self.base_dir, noun, &found);
        Ok(file.answer(text, found.is_empty()))
    }

    async fn hover(&self, args: &LspArgs) -> Result<String, String> {
        let (file, response) = self.position_request(args, "textDocument/hover", json!({})).await?;
        Ok(file.answer(render::hover_text(&response), response.is_null()))
    }

    async fn symbols(&self, args: &LspArgs) -> Result<String, String> {
        if args.path.is_some() {
            let file = self.open(args.path.as_deref()).await?;
            let response = file
                .client
                .request(
                    "textDocument/documentSymbol",
                    json!({ "textDocument": file.text_document() }),
                )
                .await?;
            let entries = render::document_symbols(&response);
            let text = match args.query.as_deref() {
                Some(query) => {
                    let matched: Vec<_> = render::matching_symbols(&entries, query).into_iter().cloned().collect();
                    render::format_outline(&matched)
                }
                None => render::format_outline(&entries),
            };
            return Ok(file.answer(text, entries.is_empty()));
        }
        let query = args
            .query
            .as_deref()
            .ok_or("symbols needs path (file outline) or query (workspace search)")?;
        let clients = self.manager.workspace_clients().await;
        if clients.is_empty() {
            return Err("no language server is running for this workspace; pass path to start one".into());
        }
        let mut merged = Vec::new();
        let mut failures = Vec::new();
        for client in clients {
            let _ = client.wait_until_idle(tokio::time::Instant::now() + READY_WAIT).await;
            let answer = match client.sync_open_documents_from_disk().await {
                Ok(()) => client.request("workspace/symbol", json!({ "query": query })).await,
                Err(error) => Err(error),
            };
            match answer {
                Ok(Value::Array(items)) => merged.extend(items),
                Ok(_) => {}
                Err(error) => failures.push(format!("{}: {error}", client.name)),
            }
        }
        let mut out = render::format_workspace_symbols(&self.base_dir, &Value::Array(merged));
        if !failures.is_empty() {
            out.push_str(&format!("\n[symbol search failed: {}]", failures.join("; ")));
        }
        Ok(out)
    }

    async fn diagnostics(&self, path: Option<&str>) -> Result<String, String> {
        if path.is_some() {
            let path = self.resolve(path)?;
            let text = tokio::fs::read_to_string(&path)
                .await
                .map_err(|error| format!("reading {}: {error}", path.display()))?;
            return crate::lsp::diagnostics_for(&self.manager, &path, &text, DIAGNOSTICS_WAIT)
                .await
                .ok_or_else(|| self.manager.unavailable_reason(&path));
        }
        let clients = self.manager.running_clients();
        if clients.is_empty() {
            return Err("no language server is running yet; pass path to check a file".into());
        }
        Ok(self.published_report(&clients))
    }

    fn published_report(&self, clients: &[Arc<LspClient>]) -> String {
        let mut sections = Vec::new();
        let mut total = Vec::new();
        for client in clients {
            for (uri, mut items) in client.state().all_published() {
                items.retain(|d| d.severity <= 2);
                if items.is_empty() {
                    continue;
                }
                render::sort_diagnostics(&mut items);
                let path = crate::lsp::client::uri_path(&uri).unwrap_or_else(|| PathBuf::from(&uri));
                sections.push(format!(
                    "{}\n{}",
                    render::display_path(&self.base_dir, &path),
                    render::diagnostic_rows(&items)
                ));
                total.extend(items);
            }
        }
        if total.is_empty() {
            return "No errors or warnings reported".into();
        }
        sections.sort();
        format!("{}\n\n{}", render::count_summary(&total), sections.join("\n\n"))
    }

    fn status(&self) -> String {
        let running = self.manager.status_lines();
        let mut out = if running.is_empty() {
            "No language server started yet (servers start when a file of their language is read or queried)"
                .to_string()
        } else {
            format!("Started:\n{}", running.join("\n"))
        };
        let servers = self.manager.servers();
        let (installed, missing): (Vec<_>, Vec<_>) =
            servers.iter().partition(|server| self.manager.is_installed(server));
        let installed: Vec<String> = installed
            .iter()
            .map(|server| format!("{} (.{})", server.name, server.extensions.join(", .")))
            .collect();
        let missing: Vec<&str> = missing.iter().map(|server| server.name.as_str()).collect();
        out.push_str(&format!("\n\nInstalled: {}", installed.join("; ")));
        if !missing.is_empty() {
            out.push_str(&format!("\nNot on PATH: {}", missing.join(", ")));
        }
        if let Some(notice) = self.manager.notice() {
            out.push_str(&format!("\nNote: {notice}"));
        }
        out
    }
}

#[async_trait::async_trait]
impl TypedTool for LspTool {
    type Args = LspArgs;
    const NAME: &'static str = "lsp";
    const CONCURRENCY_SAFE: bool = false;

    async fn run(&self, args: LspArgs) -> Result<ToolResult, AppError> {
        Ok(match self.run_action(args).await {
            Ok(text) => ToolResult::success(text),
            Err(message) => ToolResult::error(message),
        })
    }
}

mod rename;
#[cfg(test)]
mod rename_tests;
#[cfg(test)]
mod tests;
