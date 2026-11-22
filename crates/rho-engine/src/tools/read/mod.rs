pub mod format;
pub mod images;
#[cfg(test)]
mod tests;

pub use format::{format_content, is_binary, number_lines};
pub use rho_harness_core::args::ReadArgs;

use crate::lsp::{LspManager, render};
use crate::tools::engine_tool::TypedTool;
use crate::tools::types::{ToolResult, resolve_tool_path};
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::{Workspace, clean_path_arg};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct ReadTool {
    pub base_dir: PathBuf,
    pub artifact_dir: Option<PathBuf>,
    pub lsp: Option<Arc<LspManager>>,
}

async fn resolve_and_read_bytes(
    base_dir: &Path,
    clean_path: &str,
) -> std::result::Result<(PathBuf, Vec<u8>), ToolResult> {
    let workspace = Workspace::new(base_dir);
    let path = resolve_tool_path(&workspace, clean_path, "read")?;
    match tokio::fs::read(&path).await {
        Ok(bytes) => Ok((path, bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let hint = crate::tools::suggest::missing_path_hint(
                workspace.root(),
                clean_path,
                crate::tools::suggest::PathKind::File,
            )
            .await;
            Err(ToolResult::error(format!(
                "File not found: {clean_path} (in working directory: {}){hint}",
                workspace.root().display()
            )))
        }
        Err(e) => Err(ToolResult::error(format!("Failed to read {clean_path}: {e}"))),
    }
}

async fn handle_non_text_content(raw_bytes: &[u8], clean_path: &str) -> Option<ToolResult> {
    if let Some(sniffed) = images::detect_supported_image_mime(raw_bytes) {
        let bytes = raw_bytes.to_vec();
        let mime = sniffed.mime();
        return tokio::task::spawn_blocking(move || images::tool_result(&bytes, mime))
            .await
            .ok();
    }
    if is_binary(raw_bytes) {
        return Some(ToolResult::success(format!(
            "[Binary file: {} bytes, path: {}]",
            raw_bytes.len(),
            clean_path
        )));
    }
    None
}

fn is_skill_first_read(clean_path: &str, base_dir: &Path, offset: Option<usize>) -> bool {
    if offset.unwrap_or(1) > 1 {
        return false;
    }
    let path = Path::new(clean_path);
    rho_harness_core::skills::is_skill_path(path) || rho_harness_core::skills::is_skill_path(&base_dir.join(path))
}

impl ReadTool {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        let base = base_dir.as_ref().to_path_buf();
        let artifact_dir = if base.as_os_str().is_empty() {
            std::env::temp_dir().join("rho/artifacts")
        } else {
            base.join(".rho/artifacts")
        };
        Self {
            base_dir: base,
            artifact_dir: Some(artifact_dir),
            lsp: None,
        }
    }

    pub fn with_lsp(mut self, lsp: Option<Arc<LspManager>>) -> Self {
        self.lsp = lsp;
        self
    }

    pub fn with_artifact_dir(mut self, artifact_dir: Option<PathBuf>) -> Self {
        self.artifact_dir = artifact_dir;
        self
    }

    pub async fn execute(&self, mut args: ReadArgs) -> Result<ToolResult, AppError> {
        let clean_path = clean_path_arg(&args.path);
        if clean_path.is_empty() {
            return Ok(ToolResult::error("Empty file path provided for read tool"));
        }

        if is_skill_first_read(clean_path, &self.base_dir, args.offset) {
            args.limit = None;
        }

        let (path, raw_bytes) = match resolve_and_read_bytes(&self.base_dir, clean_path).await {
            Ok(read) => read,
            Err(e) => return Ok(e),
        };

        if let Some(res) = handle_non_text_content(&raw_bytes, clean_path).await {
            return Ok(res);
        }

        let Ok(content) = String::from_utf8(raw_bytes) else {
            return Ok(ToolResult::error(format!("File contains invalid UTF-8: {clean_path}")));
        };

        if let Some(symbol) = args.symbol.clone() {
            match self.symbol_lines(&path, &content, &symbol).await {
                Ok((start, end)) => {
                    args.offset = Some(start);
                    args.limit = Some(end + 1 - start);
                }
                Err(message) => return Ok(ToolResult::error(message)),
            }
        } else if let Some(lsp) = &self.lsp {
            lsp.warm(&path);
        }

        Ok(format_content(&content, &args, self.artifact_dir.as_deref()))
    }

    /// The 1-based line range of `symbol` in `path`, from the file's language server.
    async fn symbol_lines(&self, path: &Path, content: &str, symbol: &str) -> Result<(usize, usize), String> {
        let lsp = self
            .lsp
            .as_ref()
            .ok_or("symbol needs a language server, and none is enabled here")?;
        let client = lsp.client_for(path).await?;
        let language = lsp
            .servers()
            .iter()
            .find(|server| server.name == client.name)
            .map_or_else(String::new, |server| server.language_id(path));
        client.sync(path, content, &language).await?;
        let _ = client
            .wait_until_idle(tokio::time::Instant::now() + crate::tools::lsp::READY_WAIT)
            .await;
        let uri = crate::lsp::client::file_uri(path);
        let response = client
            .request(
                "textDocument/documentSymbol",
                serde_json::json!({ "textDocument": { "uri": uri } }),
            )
            .await?;
        let entries = render::document_symbols(&response);
        match render::matching_symbols(&entries, symbol).as_slice() {
            [only] => Ok((only.start as usize, only.end as usize)),
            [] => Err(format!(
                "No symbol '{symbol}' in {}; lsp symbols with path lists them",
                path.display()
            )),
            several => {
                let owned: Vec<_> = several.iter().map(|entry| (*entry).clone()).collect();
                Err(format!(
                    "'{symbol}' matches {} symbols; qualify it (e.g. Type.method) or read by offset:\n{}",
                    several.len(),
                    render::format_outline(&owned)
                ))
            }
        }
    }
}

#[async_trait::async_trait]
impl TypedTool for ReadTool {
    type Args = ReadArgs;
    const NAME: &'static str = "read";
    const CONCURRENCY_SAFE: bool = true;

    async fn run(&self, args: ReadArgs) -> Result<ToolResult, AppError> {
        self.execute(args).await
    }
}
