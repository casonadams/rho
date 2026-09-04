pub mod format;
pub mod images;
#[cfg(test)]
mod tests;

pub use format::{format_content, is_binary, number_lines};
pub use rho_harness_core::args::ReadArgs;

use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::Workspace;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use std::path::{Path, PathBuf};

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

        // Supported images attach inline blocks; sniff before the binary check
        // because PNG's IHDR length field alone already contains null bytes.
        if let Some(sniffed) = images::detect_supported_image_mime(&raw_bytes) {
            return Ok(images::tool_result(&raw_bytes, sniffed.mime()));
        }

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

        Ok(format_content(&content, clean_path, &args))
    }
}

impl Tool for ReadTool {
    const NAME: &'static str = "read";
    type Args = ReadArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Read file contents with line numbering, offset, and limit safeguards. Reads supported images (png, jpeg, gif, webp, bmp) and attaches them to the result.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<ReadArgs>()
    }

    async fn call(&self, _context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        into_rig_result(self.execute(args).await)
    }
}
