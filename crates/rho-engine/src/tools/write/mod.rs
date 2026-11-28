use crate::tools::atomic::atomic_write;
use crate::tools::engine_tool::{EngineTool, TurnToolContext, TypedTool};
use crate::tools::file_tool::FileTool;
use crate::tools::types::{ToolResult, resolve_tool_path};
pub use rho_harness_core::args::WriteArgs;
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::{Workspace, clean_path_arg};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(test)]
mod tests;

pub(crate) enum WriteOp {}
pub(crate) type WriteTool = FileTool<WriteOp>;

async fn check_not_directory(path: &Path, clean_path: &str) -> std::result::Result<(), ToolResult> {
    if tokio::fs::metadata(path).await.map(|m| m.is_dir()).unwrap_or(false) {
        return Err(ToolResult::error(format!(
            "Cannot write to {clean_path}: target path is a directory"
        )));
    }
    Ok(())
}

async fn ensure_parent_dir(path: &Path, clean_path: &str) -> std::result::Result<Vec<PathBuf>, ToolResult> {
    let Some(parent) = path.parent() else {
        return Ok(Vec::new());
    };
    let mut missing = Vec::new();
    for dir in parent.ancestors() {
        if tokio::fs::try_exists(dir).await.unwrap_or(true) {
            break;
        }
        missing.push(dir.to_path_buf());
    }
    if let Err(e) = tokio::fs::create_dir_all(parent).await {
        return Err(ToolResult::error(format!(
            "Failed to create directories for {clean_path}: {e}"
        )));
    }
    Ok(missing)
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
    pub async fn execute(&self, args: WriteArgs) -> Result<ToolResult, AppError> {
        let clean_path = clean_path_arg(&args.path);
        let base_dir = self.base_dir.get();
        let workspace = Workspace::new(&base_dir);
        let path = match resolve_tool_path(&workspace, clean_path, "write") {
            Ok(p) => p,
            Err(e) => return Ok(e),
        };
        if let Err(e) = check_not_directory(&path, clean_path).await {
            return Ok(e);
        }
        let created_dirs = match ensure_parent_dir(&path, clean_path).await {
            Ok(dirs) => dirs,
            Err(e) => return Ok(e),
        };

        let before = crate::tools::undo::read_before(&path).await;
        let created = matches!(before, Some(None));
        let result = perform_atomic_write(&path, clean_path, &args.content).await?;
        if !result.is_error
            && let (Some(history), Some(before)) = (&self.history, before)
        {
            history.record_with_dirs(&path, before, args.content.as_bytes(), created_dirs);
        }
        Ok(crate::lsp::append_diagnostics(
            result,
            self.lsp.as_ref(),
            self.inbox.as_ref(),
            &path,
            &args.content,
            created,
        )
        .await)
    }
}

#[async_trait::async_trait]
impl TypedTool for WriteTool {
    type Args = WriteArgs;
    const NAME: &'static str = "write";

    async fn run(&self, args: WriteArgs) -> Result<ToolResult, AppError> {
        self.execute(args).await
    }

    fn bind_to_turn(&self, ctx: &TurnToolContext) -> Option<Arc<dyn EngineTool>> {
        self.bound_to_turn(ctx)
    }
}
