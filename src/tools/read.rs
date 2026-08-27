use crate::error::AppError;
use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
use crate::tools::workspace::Workspace;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DEFAULT_READ_LIMIT: usize = 2000;
pub const MAX_READ_BYTES: usize = 50 * 1024; // 50 KB

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ReadArgs {
    /// Path to the file to read (relative or absolute)
    pub path: String,
    /// Line number to start reading from (1-indexed, default: 1)
    pub offset: Option<usize>,
    /// Maximum number of lines to read (default: 2000)
    pub limit: Option<usize>,
}

pub struct ReadTool {
    pub base_dir: PathBuf,
}

impl ReadTool {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    pub async fn execute(&self, args: ReadArgs) -> Result<ToolResult, AppError> {
        let clean_path = args.path.trim().trim_matches('"').trim_matches('\'');
        if clean_path.is_empty() {
            return Ok(ToolResult::error("Empty file path provided for read tool"));
        }

        let offset = args.offset.unwrap_or(1).max(1);
        let limit = args.limit.unwrap_or(DEFAULT_READ_LIMIT);

        if let Some(builtin) = crate::skills::get_builtin_skill_content(clean_path) {
            return Ok(format_content(builtin, clean_path, (offset, limit)));
        }

        let workspace = Workspace::new(&self.base_dir);
        let Some(path) = workspace.resolve(clean_path) else {
            return Ok(ToolResult::error("Empty file path provided for read tool"));
        };
        let base = workspace.root();

        if !path.exists() {
            return Ok(ToolResult::error(format!(
                "File not found: {} (in working directory: {})",
                clean_path,
                base.display()
            )));
        }

        let raw_bytes = match tokio::fs::read(&path).await {
            Ok(b) => b,
            Err(e) => return Ok(ToolResult::error(format!("Failed to read {clean_path}: {e}"))),
        };

        // Binary check
        if is_binary(&raw_bytes) {
            return Ok(ToolResult::success(format!(
                "[Binary file: {} bytes, path: {}]",
                raw_bytes.len(),
                clean_path
            )));
        }

        let content = match String::from_utf8(raw_bytes) {
            Ok(s) => s,
            Err(_) => return Ok(ToolResult::error(format!("File contains invalid UTF-8: {clean_path}"))),
        };

        Ok(format_content(&content, clean_path, (offset, limit)))
    }
}

fn format_content(content: &str, clean_path: &str, range: (usize, usize)) -> ToolResult {
    let (offset, limit) = range;
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    if offset > total_lines && total_lines > 0 {
        return ToolResult::success(format!(
            "[Offset {} exceeds total lines {} in {}]",
            offset, total_lines, clean_path
        ));
    }

    let start_idx = offset - 1;
    let end_idx = (start_idx + limit).min(total_lines);

    let mut output = String::new();
    let mut total_bytes = 0;
    let mut truncated = false;

    for (idx, line) in lines[start_idx..end_idx].iter().enumerate() {
        let line_num = start_idx + idx + 1;
        let formatted_line = format!("{line_num:6}\t{line}\n");

        if total_bytes + formatted_line.len() > MAX_READ_BYTES {
            truncated = true;
            output.push_str(&format!(
                "\n[Truncated at {} KB limit. Total lines: {}, read up to line {}]",
                MAX_READ_BYTES / 1024,
                total_lines,
                line_num - 1
            ));
            break;
        }

        output.push_str(&formatted_line);
        total_bytes += formatted_line.len();
    }

    if !truncated && end_idx < total_lines {
        output.push_str(&format!(
            "\n[Lines {}-{} of {} lines in {}]",
            offset, end_idx, total_lines, clean_path
        ));
    }

    ToolResult::success(output)
}

fn is_binary(bytes: &[u8]) -> bool {
    let check_len = bytes.len().min(1024);
    bytes[..check_len].contains(&0)
}

impl Tool for ReadTool {
    const NAME: &'static str = "read";
    type Args = ReadArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Read file contents with line numbering, offset, and limit safeguards.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<ReadArgs>()
    }

    async fn call(&self, _context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        into_rig_result(self.execute(args).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_read_tool_happy_path() {
        let temp_dir = std::env::temp_dir().join(format!("read_test_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let file_path = temp_dir.join("sample.txt");
        tokio::fs::write(&file_path, "line1\nline2\nline3\n").await.unwrap();

        let tool = ReadTool::new(&temp_dir);
        let res = tool
            .execute(ReadArgs {
                path: file_path.to_str().unwrap().to_string(),
                offset: Some(1),
                limit: Some(2),
            })
            .await
            .unwrap();

        assert!(!res.is_error);
        assert!(res.content.contains("line1"));
        assert!(res.content.contains("line2"));
        assert!(!res.content.contains("line3"));

        let _ = tokio::fs::remove_dir_all(temp_dir).await;
    }

    #[tokio::test]
    async fn test_read_truncates_at_byte_limit() {
        let temp_dir = std::env::temp_dir().join(format!("read_test_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let file_path = temp_dir.join("large.txt");
        tokio::fs::write(&file_path, "x".repeat(MAX_READ_BYTES * 2))
            .await
            .unwrap();

        let result = ReadTool::new(&temp_dir)
            .execute(ReadArgs {
                path: file_path.to_string_lossy().into_owned(),
                offset: None,
                limit: None,
            })
            .await
            .unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("Truncated at 50 KB limit"));
        assert!(result.content.len() <= MAX_READ_BYTES + 200);
        let _ = tokio::fs::remove_dir_all(temp_dir).await;
    }

    #[tokio::test]
    async fn test_read_missing_file() {
        let tool = ReadTool::new(std::env::temp_dir());
        let res = tool
            .execute(ReadArgs {
                path: "nonexistent_file_xyz_123.txt".to_string(),
                offset: None,
                limit: None,
            })
            .await
            .unwrap();

        assert!(res.is_error);
        assert!(res.content.contains("File not found"));
    }

    #[tokio::test]
    async fn test_read_builtin_embedded_skill() {
        let tool = ReadTool::new(std::env::temp_dir());
        let res = tool
            .execute(ReadArgs {
                path: "rho://skills/create-plugin".to_string(),
                offset: None,
                limit: None,
            })
            .await
            .unwrap();

        assert!(!res.is_error);
        assert!(res.content.contains("Creating a Plugin for `rho`"));
        assert!(res.content.contains("rho::plugin::Extension"));
    }
}
