use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
use rho_core::approval::enforce_approval;
pub use rho_core::args::WriteArgs;
use rho_core::error::AppError;
use rho_core::workspace::Workspace;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use std::path::{Path, PathBuf};

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
        match tokio::fs::write(&path, &args.content).await {
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

    async fn call(&self, context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        enforce_approval(context, Self::NAME, &args)?;
        into_rig_result(self.execute(args).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_excluded_targets_before_writing() {
        let temp_dir = std::env::temp_dir().join(format!("write_test_{}", uuid::Uuid::new_v4()));
        let excluded = temp_dir.join("rho");
        tokio::fs::create_dir_all(&excluded).await.unwrap();
        let path = excluded.join("config.toml");
        let tool = WriteTool::with_exclusions(&temp_dir, [&excluded]);
        let result = tool
            .execute(WriteArgs {
                path: path.to_string_lossy().into_owned(),
                content: "secret = true".to_string(),
            })
            .await
            .unwrap();
        assert!(result.is_error);
        assert!(!path.exists());
        let _ = tokio::fs::remove_dir_all(temp_dir).await;
    }

    #[tokio::test]
    async fn test_write_tool() {
        let temp_dir = std::env::temp_dir().join(format!("write_test_{}", uuid::Uuid::new_v4()));
        let tool = WriteTool::new(&temp_dir);
        let file_path = temp_dir.join("sub/nested/file.txt");
        let res = tool
            .execute(WriteArgs {
                path: file_path.to_str().unwrap().to_string(),
                content: "hello world\nsecond line\n".to_string(),
            })
            .await
            .unwrap();
        assert!(!res.is_error);
        assert!(res.content.contains("Successfully wrote"));
        let disk_content = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(disk_content, "hello world\nsecond line\n");
        let _ = tokio::fs::remove_dir_all(temp_dir).await;
    }
}
