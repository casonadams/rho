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
        let Some(path) = workspace.resolve(clean_path) else {
            return Ok(ToolResult::error("Empty file path provided for write tool"));
        };
        if !workspace.can_mutate(clean_path) {
            return Ok(ToolResult::error(format!(
                "Write target is outside the permitted workspace: {clean_path}"
            )));
        }
        if path.is_dir() {
            return Ok(ToolResult::error(format!(
                "Cannot write to {clean_path}: target path is a directory"
            )));
        }
        if let Some(parent) = path.parent()
            && let Err(e) = tokio::fs::create_dir_all(parent).await
        {
            return Ok(ToolResult::error(format!(
                "Failed to create directories for {clean_path}: {e}"
            )));
        }

        if !workspace.can_mutate(clean_path) {
            return Ok(ToolResult::error(format!(
                "Write target moved outside the permitted workspace: {clean_path}"
            )));
        }

        let bytes_len = args.content.len();
        let lines_len = args.content.lines().count();
        match atomic_write(&path, args.content.as_bytes()).await {
            Ok(_) => Ok(ToolResult::success(format!(
                "Successfully wrote {} bytes ({} lines) to {}",
                bytes_len, lines_len, clean_path
            ))),
            Err(e) => Ok(ToolResult::error(format!("Failed to write file {clean_path}: {e}"))),
        }
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
