pub mod format;
pub mod images;
#[cfg(test)]
mod tests;
mod window;

pub(crate) use format::{format_content, is_binary};
pub use rho_harness_core::args::ReadArgs;
use window::LineWindow;

use crate::lsp::{LspManager, render};
use crate::tools::engine_tool::TypedTool;
use crate::tools::types::{ToolResult, resolve_tool_path};
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::{Workspace, clean_path_arg};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MAX_WHOLE_READ_BYTES: u64 = 16 * 1024 * 1024;

pub(crate) struct ReadTool {
    pub base_dir: crate::tools::WorkspaceDir,
    pub artifact_dir: Option<PathBuf>,
    initial_default_artifact_dir: PathBuf,
    pub lsp: Option<Arc<LspManager>>,
}

/// The path `clean_path` names and its size, when it is a regular file.
async fn resolve_regular_file(base_dir: &Path, clean_path: &str) -> std::result::Result<(PathBuf, u64), ToolResult> {
    let workspace = Workspace::new(base_dir);
    let path = resolve_tool_path(&workspace, clean_path, "read")?;
    match tokio::fs::metadata(&path).await {
        Ok(metadata) if metadata.is_file() => Ok((path, metadata.len())),
        Ok(metadata) if metadata.is_dir() => Err(ToolResult::error(format!(
            "Cannot read {clean_path}: it is a directory"
        ))),
        Ok(_) => Err(ToolResult::error(format!(
            "Cannot read {clean_path}: not a regular file"
        ))),
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

async fn process_image(clean_path: &str, decode: impl FnOnce() -> ToolResult + Send + 'static) -> ToolResult {
    tokio::task::spawn_blocking(decode)
        .await
        .unwrap_or_else(|e| ToolResult::error(format!("Failed to process image {clean_path}: {e}")))
}

fn binary_marker(size: u64, clean_path: &str) -> ToolResult {
    ToolResult::success(format!("[Binary file: {size} bytes, path: {clean_path}]"))
}

enum Sniffed {
    Shown(ToolResult),
    Text(Vec<u8>),
}

async fn sniff_content(raw_bytes: Vec<u8>, clean_path: &str) -> Sniffed {
    if let Some(sniffed) = images::detect_supported_image_mime(&raw_bytes) {
        let mime = sniffed.mime();
        return Sniffed::Shown(process_image(clean_path, move || images::tool_result(&raw_bytes, mime)).await);
    }
    if is_binary(&raw_bytes) {
        return Sniffed::Shown(binary_marker(raw_bytes.len() as u64, clean_path));
    }
    Sniffed::Text(raw_bytes)
}

/// Reads a file too large to load whole: a binary file is described from its first bytes and a
/// text file streams only `window`. `None` for an image, which is decoded whole.
fn read_large(path: &Path, size: u64, window: LineWindow, clean_path: &str) -> std::io::Result<Option<ToolResult>> {
    let mut reader = std::io::BufReader::with_capacity(64 * 1024, std::fs::File::open(path)?);
    let head = std::io::BufRead::fill_buf(&mut reader)?;
    if images::detect_supported_image_mime(head).is_some() {
        return Ok(None);
    }
    if is_binary(head) {
        return Ok(Some(binary_marker(size, clean_path)));
    }
    window::read_window(reader, size, window, clean_path).map(Some)
}

async fn read_large_off_thread(path: PathBuf, size: u64, args: &ReadArgs, clean_path: &str) -> Option<ToolResult> {
    if args.symbol.is_some() {
        return Some(ToolResult::error(format!(
            "{clean_path} is too large to read by symbol; use offset and limit"
        )));
    }
    let window = LineWindow {
        offset: args.offset,
        limit: args.limit,
    };
    let owned_path = clean_path.to_string();
    let read = tokio::task::spawn_blocking(move || read_large(&path, size, window, &owned_path)).await;
    match read {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => Some(ToolResult::error(format!("Failed to read {clean_path}: {e}"))),
        Err(e) => Some(ToolResult::error(format!("Failed to read {clean_path}: {e}"))),
    }
}

fn default_artifact_dir(base: &Path) -> PathBuf {
    if base.as_os_str().is_empty() {
        std::env::temp_dir().join("rho/artifacts")
    } else {
        base.join(".rho/artifacts")
    }
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
        let artifact_dir = default_artifact_dir(&base);
        Self {
            base_dir: crate::tools::WorkspaceDir::new(base),
            artifact_dir: Some(artifact_dir.clone()),
            initial_default_artifact_dir: artifact_dir,
            lsp: None,
        }
    }

    pub(crate) fn with_workspace_dir(mut self, base_dir: crate::tools::WorkspaceDir) -> Self {
        self.base_dir = base_dir;
        self
    }

    pub(crate) fn with_lsp(mut self, lsp: Option<Arc<LspManager>>) -> Self {
        self.lsp = lsp;
        self
    }

    fn effective_artifact_dir(&self, base_dir: &Path) -> Option<PathBuf> {
        match &self.artifact_dir {
            Some(dir) if *dir == self.initial_default_artifact_dir => Some(default_artifact_dir(base_dir)),
            other => other.clone(),
        }
    }

    pub async fn execute(&self, mut args: ReadArgs) -> Result<ToolResult, AppError> {
        let clean_path = clean_path_arg(&args.path);
        if clean_path.is_empty() {
            return Ok(ToolResult::error("Empty file path provided for read tool"));
        }

        if args.symbol.is_some() && (args.offset.is_some() || args.limit.is_some()) {
            return Ok(ToolResult::error(
                "symbol cannot be combined with offset or limit; pass one or the other",
            ));
        }

        let base_dir = self.base_dir.get();
        if is_skill_first_read(clean_path, &base_dir, args.offset) {
            args.limit = None;
        }

        let (path, size) = match resolve_regular_file(&base_dir, clean_path).await {
            Ok(file) => file,
            Err(e) => return Ok(e),
        };
        if size > MAX_WHOLE_READ_BYTES
            && let Some(result) = read_large_off_thread(path.clone(), size, &args, clean_path).await
        {
            return Ok(result);
        }

        let raw_bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(e) => return Ok(ToolResult::error(format!("Failed to read {clean_path}: {e}"))),
        };
        let raw_bytes = match sniff_content(raw_bytes, clean_path).await {
            Sniffed::Shown(result) => return Ok(result),
            Sniffed::Text(bytes) => bytes,
        };
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

        let artifact_dir = self.effective_artifact_dir(&base_dir);
        Ok(format_content(&content, &args, artifact_dir.as_deref()))
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
