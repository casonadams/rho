use crate::tools::atomic::atomic_write;
use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
pub use rho_harness_core::args::WriteArgs;
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::Workspace;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

pub struct WriteTool {
    pub base_dir: PathBuf,
    exclusions: Vec<PathBuf>,
}

fn validate_write_workspace(
    workspace: &Workspace,
    clean_path: &str,
) -> std::result::Result<std::path::PathBuf, ToolResult> {
    let Some(path) = workspace.resolve(clean_path) else {
        return Err(ToolResult::error("Empty file path provided for write tool"));
    };
    if !workspace.can_mutate(clean_path) {
        return Err(ToolResult::error(format!(
            "Write target is outside the permitted workspace: {clean_path}"
        )));
    }
    Ok(path)
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
        Self::with_exclusions(base_dir, std::iter::empty::<&Path>())
    }

    pub fn with_exclusions<I, P>(base_dir: impl AsRef<Path>, exclusions: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
            exclusions: exclusions.into_iter().map(|path| path.as_ref().to_path_buf()).collect(),
        }
    }

    pub async fn execute(&self, args: WriteArgs) -> Result<ToolResult, AppError> {
        let clean_path = args.path.trim().trim_matches('"').trim_matches('\'');
        if clean_path.is_empty() {
            return Ok(ToolResult::error("Empty file path provided for write tool"));
        }

        let workspace = Workspace::with_exclusions(&self.base_dir, &self.exclusions);
        let path = match validate_write_workspace(&workspace, clean_path) {
            Ok(p) => p,
            Err(e) => return Ok(e),
        };
        if let Err(e) = check_not_directory(&path, clean_path).await {
            return Ok(e);
        }
        if let Err(e) = ensure_parent_dir(&path, clean_path).await {
            return Ok(e);
        }
        if !workspace.can_mutate(clean_path) {
            return Ok(ToolResult::error(format!(
                "Write target moved outside the permitted workspace: {clean_path}"
            )));
        }

        perform_atomic_write(&path, clean_path, &args.content).await
    }
}

impl Tool for WriteTool {
    const NAME: &'static str = "write";
    type Args = WriteArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Write full content to a file, automatically creating parent directories.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<WriteArgs>()
    }

    async fn call(&self, _context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        into_rig_result(self.execute(args).await)
    }
}
