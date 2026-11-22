use crate::lsp::LspManager;
use crate::tools::atomic::atomic_write;
use crate::tools::engine_tool::TypedTool;
use crate::tools::types::{ToolResult, resolve_tool_path};
pub use rho_harness_core::args::WriteArgs;
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::{Workspace, clean_path_arg};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(test)]
mod tests;

pub struct WriteTool {
    pub base_dir: PathBuf,
    pub lsp: Option<Arc<LspManager>>,
}

async fn check_not_directory(path: &Path, clean_path: &str) -> std::result::Result<(), ToolResult> {
    if tokio::fs::metadata(path).await.map(|m| m.is_dir()).unwrap_or(false) {
        return Err(ToolResult::error(format!(
            "Cannot write to {clean_path}: target path is a directory"
        )));
    }
    Ok(())
}

async fn ensure_parent_dir(path: &Path, clean_path: &str) -> std::result::Result<(), ToolResult> {
    if let Some(parent) = path.parent()
        && let Err(e) = tokio::fs::create_dir_all(parent).await
    {
        return Err(ToolResult::error(format!(
            "Failed to create directories for {clean_path}: {e}"
        )));
    }
    Ok(())
}

async fn perform_atomic_write(path: &Path, clean_path: &str, content: &str) -> Result<ToolResult, AppError> {
    let bytes_len = content.len();
    let lines_len = content.lines().count();
    match atomic_write(path, content.as_bytes()).await {
        Ok(_) => Ok(ToolResult::success(format!(
            "Successfully wrote {bytes_len} bytes ({lines_len} lines) to {clean_path}"
        ))),
        Err(e) => Ok(ToolResult::error(format!("Failed to write file {clean_path}: {e}"))),
    }
}

impl WriteTool {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
            lsp: None,
        }
    }

    pub fn with_lsp(mut self, lsp: Option<Arc<LspManager>>) -> Self {
        self.lsp = lsp;
        self
    }

    pub async fn execute(&self, args: WriteArgs) -> Result<ToolResult, AppError> {
        let clean_path = clean_path_arg(&args.path);
        let workspace = Workspace::new(&self.base_dir);
        let path = match resolve_tool_path(&workspace, clean_path, "write") {
            Ok(p) => p,
            Err(e) => return Ok(e),
        };
        if let Err(e) = check_not_directory(&path, clean_path).await {
            return Ok(e);
        }
        if let Err(e) = ensure_parent_dir(&path, clean_path).await {
            return Ok(e);
        }

        let result = perform_atomic_write(&path, clean_path, &args.content).await?;
        Ok(crate::lsp::append_diagnostics(result, self.lsp.as_deref(), &path, &args.content).await)
    }
}

#[async_trait::async_trait]
impl TypedTool for WriteTool {
    type Args = WriteArgs;
    const NAME: &'static str = "write";

    async fn run(&self, args: WriteArgs) -> Result<ToolResult, AppError> {
        self.execute(args).await
    }
}
