use crate::lsp::manager::LspManager;
use crate::lsp::{LspClient, LspLocation, LspRange, LspSymbol, symbol_kind_name};
use crate::tools::engine_tool::EngineTool;
use crate::tools::types::{ToolResult, generated_schema};
use rho_harness_core::args::LspArgs;
use rho_harness_core::error::AppError;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct TargetCoords {
    client: Arc<LspClient>,
    abs_path: PathBuf,
    uri: String,
    line: usize,
    col: usize,
}

pub struct LspTool {
    base_dir: PathBuf,
    manager: Arc<LspManager>,
}

impl LspTool {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        let base_dir = base_dir.into();
        let manager = Arc::new(LspManager::new(&base_dir));
        Self { base_dir, manager }
    }

    pub fn with_client(base_dir: impl Into<PathBuf>, client: Arc<LspClient>) -> Self {
        let manager = Arc::new(LspManager::with_client(base_dir, client));
        let base_dir = manager.base_dir().to_path_buf();
        Self { base_dir, manager }
    }

    fn resolve_position(
        &self,
        path: Option<String>,
        line: Option<usize>,
        char: Option<usize>,
    ) -> std::result::Result<(PathBuf, usize, usize), String> {
        let p = match path {
            Some(p) if !p.trim().is_empty() => p,
            _ => return Err("path is required".to_string()),
        };
        let l = line.unwrap_or(1);
        let c = char.unwrap_or(1);
        let abs_path = if Path::new(&p).is_absolute() {
            PathBuf::from(&p)
        } else {
            self.base_dir.join(&p)
        };
        Ok((abs_path, l, c))
    }

    async fn resolve_target_coords(
        &self,
        path: Option<String>,
        line: Option<usize>,
        char: Option<usize>,
    ) -> Result<std::result::Result<TargetCoords, ToolResult>, AppError> {
        let (abs_path, line, col) = match self.resolve_position(path, line, char) {
            Ok(c) => c,
            Err(e) => return Ok(Err(ToolResult::error(e))),
        };
        let client = match self.manager.get_client().await {
            Ok(c) => c,
            Err(e) => return Ok(Err(ToolResult::error(e.to_string()))),
        };
        let uri = match url::Url::from_file_path(&abs_path) {
            Ok(u) => u.to_string(),
            Err(_) => return Ok(Err(ToolResult::error("failed to create file URI from path"))),
        };
        Ok(Ok(TargetCoords {
            client,
            abs_path,
            uri,
            line,
            col,
        }))
    }

    fn format_locations(&self, locs: &[LspLocation]) -> String {
        let mut lines = Vec::new();
        for loc in locs {
            let path_str = url_to_display_path(&loc.uri, &self.base_dir);
            let line = loc.range.start.line + 1;
            let col = loc.range.start.character + 1;
            lines.push(format!("{path_str}:{line}:{col}"));
        }
        lines.join("\n")
    }

    fn format_workspace_edit(&self, edit: &serde_json::Value, new_name: &str) -> String {
        let mut summary_lines = Vec::new();
        summary_lines.push(format!("Proposed rename edits to '{new_name}':"));

        if let Some(changes) = edit.get("changes").and_then(|c| c.as_object()) {
            for (uri, edits) in changes {
                let path_str = url_to_display_path(uri, &self.base_dir);
                let count = edits.as_array().map(|a| a.len()).unwrap_or(0);
                summary_lines.push(format!("  {path_str}: {count} edit(s)"));
            }
        } else if let Some(doc_changes) = edit.get("documentChanges").and_then(|d| d.as_array()) {
            for dc in doc_changes {
                if let Some(uri) = dc.pointer("/textDocument/uri").and_then(|u| u.as_str()) {
                    let path_str = url_to_display_path(uri, &self.base_dir);
                    let count = dc.get("edits").and_then(|e| e.as_array()).map(|a| a.len()).unwrap_or(0);
                    summary_lines.push(format!("  {path_str}: {count} edit(s)"));
                }
            }
        } else {
            summary_lines.push("  No text edits found in workspace edit".to_string());
        }

        summary_lines.join("\n")
    }

    fn op_detect(&self) -> ToolResult {
        if let Some((server, sargs)) = self.manager.auto_detect_command() {
            let cmd_str = format!("{server} {}", sargs.join(" ")).trim().to_string();
            ToolResult::success(format!("Detected LSP server for workspace: {cmd_str}"))
        } else {
            ToolResult::success("No standard LSP server auto-detected for workspace root".to_string())
        }
    }

    async fn notify_file_if_exists(&self, client: &LspClient, abs_path: &Path) {
        if abs_path.exists()
            && let Ok(content) = std::fs::read_to_string(abs_path)
        {
            let _ = client.notify_did_open(abs_path, &content).await;
        }
    }

    async fn diagnostics_for_file(client: &LspClient, abs_path: &Path, base_dir: &Path) -> ToolResult {
        let diags = client.get_diagnostics(Some(abs_path)).await;
        let rel = abs_path.strip_prefix(base_dir).unwrap_or(abs_path);
        let path_str = rel.display().to_string();
        let mut out = Vec::new();
        for (_uri, list) in diags {
            format_diagnostic_items(&path_str, &list, &mut out);
        }
        if out.is_empty() {
            ToolResult::success(format!("No diagnostics found for {path_str} (0 errors, 0 warnings)."))
        } else {
            ToolResult::success(out.join("\n"))
        }
    }

    async fn diagnostics_for_workspace(client: &LspClient, base_dir: &Path) -> ToolResult {
        let diags = client.get_diagnostics(None).await;
        let mut out = Vec::new();
        for (uri, list) in diags {
            let path_str = url_to_display_path(&uri, base_dir);
            format_diagnostic_items(&path_str, &list, &mut out);
        }
        if out.is_empty() {
            ToolResult::success("Workspace clean: 0 diagnostics reported.".to_string())
        } else {
            ToolResult::success(out.join("\n"))
        }
    }

    async fn op_diagnostics(&self, path: Option<String>) -> Result<ToolResult, AppError> {
        let client = match self.manager.get_client().await {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::error(e.to_string())),
        };

        if let Some(p) = path {
            let abs_path = if Path::new(&p).is_absolute() {
                PathBuf::from(&p)
            } else {
                self.base_dir.join(&p)
            };
            self.notify_file_if_exists(&client, &abs_path).await;
            Ok(Self::diagnostics_for_file(&client, &abs_path, &self.base_dir).await)
        } else {
            Ok(Self::diagnostics_for_workspace(&client, &self.base_dir).await)
        }
    }

    async fn op_definition(
        &self,
        path: Option<String>,
        line: Option<usize>,
        char: Option<usize>,
    ) -> Result<ToolResult, AppError> {
        let target = match self.resolve_target_coords(path, line, char).await? {
            Ok(t) => t,
            Err(err) => return Ok(err),
        };
        let params = serde_json::json!({
            "textDocument": { "uri": target.uri },
            "position": { "line": target.line.saturating_sub(1), "character": target.col.saturating_sub(1) }
        });
        let res = match target.client.send_request("textDocument/definition", params).await {
            Ok(v) => v,
            Err(e) => return Ok(ToolResult::error(e.to_string())),
        };
        let locs = parse_locations(&res);
        if locs.is_empty() {
            let rel = target.abs_path.strip_prefix(&self.base_dir).unwrap_or(&target.abs_path);
            Ok(ToolResult::success(format!(
                "No definition found for symbol at {}:{}:{}",
                rel.display(),
                target.line,
                target.col
            )))
        } else {
            Ok(ToolResult::success(self.format_locations(&locs)))
        }
    }

    async fn op_references(
        &self,
        path: Option<String>,
        line: Option<usize>,
        char: Option<usize>,
    ) -> Result<ToolResult, AppError> {
        let target = match self.resolve_target_coords(path, line, char).await? {
            Ok(t) => t,
            Err(err) => return Ok(err),
        };
        let params = serde_json::json!({
            "textDocument": { "uri": target.uri },
            "position": { "line": target.line.saturating_sub(1), "character": target.col.saturating_sub(1) },
            "context": { "includeDeclaration": true }
        });
        let res = match target.client.send_request("textDocument/references", params).await {
            Ok(v) => v,
            Err(e) => return Ok(ToolResult::error(e.to_string())),
        };
        let locs = parse_locations(&res);
        if locs.is_empty() {
            let rel = target.abs_path.strip_prefix(&self.base_dir).unwrap_or(&target.abs_path);
            Ok(ToolResult::success(format!(
                "No references found for symbol at {}:{}:{}",
                rel.display(),
                target.line,
                target.col
            )))
        } else {
            Ok(ToolResult::success(self.format_locations(&locs)))
        }
    }

    async fn op_symbols(&self, path: Option<String>) -> Result<ToolResult, AppError> {
        let p = match path {
            Some(p) if !p.trim().is_empty() => p,
            _ => return Ok(ToolResult::error("path is required for 'symbols' operation")),
        };
        let abs_path = if Path::new(&p).is_absolute() {
            PathBuf::from(&p)
        } else {
            self.base_dir.join(&p)
        };
        let client = match self.manager.get_client().await {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::error(e.to_string())),
        };
        let uri = match url::Url::from_file_path(&abs_path) {
            Ok(u) => u.to_string(),
            Err(_) => return Ok(ToolResult::error("failed to create file URI from path")),
        };
        let params = serde_json::json!({
            "textDocument": { "uri": uri }
        });
        let res = match client.send_request("textDocument/documentSymbol", params).await {
            Ok(v) => v,
            Err(e) => return Ok(ToolResult::error(e.to_string())),
        };
        let symbols = parse_symbols(&res);
        let rel = abs_path.strip_prefix(&self.base_dir).unwrap_or(&abs_path);
        if symbols.is_empty() {
            Ok(ToolResult::success(format!("No symbols found in {}", rel.display())))
        } else {
            let mut lines = Vec::new();
            for s in symbols {
                let kind = symbol_kind_name(s.kind);
                let start_line = s.range.start.line + 1;
                let end_line = s.range.end.line + 1;
                lines.push(format!("{kind}: {} (lines {start_line}-{end_line})", s.name));
            }
            Ok(ToolResult::success(lines.join("\n")))
        }
    }

    async fn op_rename(
        &self,
        path: Option<String>,
        line: Option<usize>,
        char: Option<usize>,
        new_name: Option<String>,
    ) -> Result<ToolResult, AppError> {
        let new_name = match new_name {
            Some(n) if !n.trim().is_empty() => n.trim().to_string(),
            _ => return Ok(ToolResult::error("new_name is required for 'rename' operation")),
        };
        let target = match self.resolve_target_coords(path, line, char).await? {
            Ok(t) => t,
            Err(err) => return Ok(err),
        };
        let params = serde_json::json!({
            "textDocument": { "uri": target.uri },
            "position": { "line": target.line.saturating_sub(1), "character": target.col.saturating_sub(1) },
            "newName": new_name
        });
        let res = match target.client.send_request("textDocument/rename", params).await {
            Ok(v) => v,
            Err(e) => return Ok(ToolResult::error(e.to_string())),
        };
        let summary = self.format_workspace_edit(&res, &new_name);
        Ok(ToolResult::success(summary))
    }

    pub async fn execute_lsp(&self, args: LspArgs) -> Result<ToolResult, AppError> {
        let op = args.operation.trim().to_lowercase();
        match op.as_str() {
            "detect" => Ok(self.op_detect()),
            "diagnostics" => self.op_diagnostics(args.path).await,
            "definition" => self.op_definition(args.path, args.line, args.character).await,
            "references" => self.op_references(args.path, args.line, args.character).await,
            "symbols" => self.op_symbols(args.path).await,
            "rename" => {
                self.op_rename(args.path, args.line, args.character, args.new_name)
                    .await
            }
            other => Ok(ToolResult::error(format!(
                "Unsupported LSP operation: '{other}'. Supported operations: detect, diagnostics, definition, references, symbols, rename"
            ))),
        }
    }
}

fn format_diagnostic_items(path_display: &str, items: &[crate::lsp::LspDiagnostic], out: &mut Vec<String>) {
    for d in items {
        let line = d.range.start.line + 1;
        let col = d.range.start.character + 1;
        let src = d.source.as_deref().unwrap_or("lsp");
        out.push(format!(
            "{path_display}:{line}:{col}: [{}] {} ({src})",
            d.severity_str(),
            d.message
        ));
    }
}

fn url_to_display_path(uri: &str, base_dir: &Path) -> String {
    if let Ok(u) = url::Url::parse(uri)
        && let Ok(p) = u.to_file_path()
    {
        return p.strip_prefix(base_dir).unwrap_or(&p).display().to_string();
    }
    uri.to_string()
}

fn parse_single_location(val: &serde_json::Value) -> Option<LspLocation> {
    if let (Some(uri), Some(range_val)) = (val.get("uri").and_then(|u| u.as_str()), val.get("range")) {
        let range: LspRange = serde_json::from_value(range_val.clone()).ok()?;
        return Some(LspLocation {
            uri: uri.to_string(),
            range,
        });
    }
    if let (Some(uri), Some(range_val)) = (
        val.get("targetUri").and_then(|u| u.as_str()),
        val.get("targetSelectionRange").or_else(|| val.get("targetRange")),
    ) {
        let range: LspRange = serde_json::from_value(range_val.clone()).ok()?;
        return Some(LspLocation {
            uri: uri.to_string(),
            range,
        });
    }
    None
}

fn parse_locations(val: &serde_json::Value) -> Vec<LspLocation> {
    if let Some(arr) = val.as_array() {
        arr.iter().filter_map(parse_single_location).collect()
    } else if let Some(loc) = parse_single_location(val) {
        vec![loc]
    } else {
        Vec::new()
    }
}

fn parse_symbols(val: &serde_json::Value) -> Vec<LspSymbol> {
    let mut out = Vec::new();
    if let Some(arr) = val.as_array() {
        for item in arr {
            collect_symbol(item, &mut out);
        }
    }
    out
}

fn collect_symbol(item: &serde_json::Value, out: &mut Vec<LspSymbol>) {
    let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
    let kind = item.get("kind").and_then(|k| k.as_u64()).unwrap_or(0);
    let range_val = item.get("range").or_else(|| item.pointer("/location/range"));
    if let Some(rv) = range_val
        && let Ok(range) = serde_json::from_value::<LspRange>(rv.clone())
    {
        out.push(LspSymbol { name, kind, range });
    }
    if let Some(children) = item.get("children").and_then(|c| c.as_array()) {
        for child in children {
            collect_symbol(child, out);
        }
    }
}

#[async_trait::async_trait]
impl EngineTool for LspTool {
    fn name(&self) -> &str {
        "lsp"
    }

    fn description(&self) -> &str {
        "Language Server Protocol tool for querying diagnostics, symbol definitions, references, document symbols, and renames."
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<LspArgs>()
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, AppError> {
        let args: LspArgs = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::error(format!("failed to parse tool arguments: {e}"))),
        };
        self.execute_lsp(args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_lsp_tool_detect() {
        let cwd = std::env::current_dir().unwrap();
        let tool = LspTool::new(&cwd);
        let res = tool
            .execute_lsp(LspArgs {
                operation: "detect".to_string(),
                path: None,
                line: None,
                character: None,
                new_name: None,
            })
            .await
            .unwrap();
        assert!(!res.is_error);
        assert!(res.content.contains("Detected LSP server"));
    }
}

#[path = "lsp_mock_tests.rs"]
#[cfg(test)]
mod mock_tests;
