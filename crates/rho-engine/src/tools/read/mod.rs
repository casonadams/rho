use crate::tools::truncate::{DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES, TruncatedBy, format_size, truncate_head};
use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
pub use rho_harness_core::args::ReadArgs;
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::Workspace;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use std::path::{Path, PathBuf};

pub mod images;
#[cfg(test)]
mod tests;

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

        if let Some(builtin) = rho_harness_core::skills::get_builtin_skill_content(clean_path) {
            return Ok(format_content(builtin, clean_path, &args));
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

/// pi's read-tool text branch: slice from the 1-indexed offset, truncate the
/// selection with the shared head truncator, then assemble numbered output
/// with actionable continuation notices.
fn format_content(content: &str, clean_path: &str, args: &ReadArgs) -> ToolResult {
    let offset = args.offset.unwrap_or(1).max(1);
    let user_limit = args.limit;
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();
    let start_idx = offset.saturating_sub(1);

    if start_idx >= total_lines {
        return ToolResult::error(format!(
            "Offset {offset} is beyond end of file ({total_lines} lines total)"
        ));
    }

    let selected = match user_limit {
        Some(limit) => lines[start_idx..(start_idx + limit).min(total_lines)].join("\n"),
        None => lines[start_idx..].join("\n"),
    };

    let truncation = truncate_head(&selected, DEFAULT_MAX_LINES, DEFAULT_MAX_BYTES);
    let start_line = start_idx + 1;

    if truncation.first_line_exceeds_limit {
        return ToolResult::success(format!(
            "[Line {start_line} is {}, exceeds {} limit. Use bash: sed -n '{start_line}p' {clean_path} | head -c {DEFAULT_MAX_BYTES}]",
            format_size(lines[start_idx].len()),
            format_size(DEFAULT_MAX_BYTES),
        ));
    }

    let mut output = number_lines(&truncation.content, start_line);

    if let Some(truncated_by) = truncation.truncated_by {
        let end_line = start_line + truncation.output_lines - 1;
        let next_offset = end_line + 1;
        match truncated_by {
            TruncatedBy::Lines => output.push_str(&format!(
                "\n\n[Showing lines {start_line}-{end_line} of {total_lines}. Use offset={next_offset} to continue.]"
            )),
            TruncatedBy::Bytes => output.push_str(&format!(
                "\n\n[Showing lines {start_line}-{end_line} of {total_lines} ({} limit). Use offset={next_offset} to continue.]",
                format_size(DEFAULT_MAX_BYTES)
            )),
        }
    } else if let Some(limit) = user_limit {
        let remaining = total_lines.saturating_sub(start_idx + limit);
        if remaining > 0 {
            let next_offset = start_idx + limit + 1;
            output.push_str(&format!(
                "\n\n[{remaining} more lines in file. Use offset={next_offset} to continue.]"
            ));
        }
    }

    ToolResult::success(output)
}

fn number_lines(content: &str, start_line: usize) -> String {
    let mut output = String::new();
    for (idx, line) in content.lines().enumerate() {
        let line_num = start_line + idx;
        output.push_str(&format!("{line_num:6}\t{line}\n"));
    }
    output
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
        "Read file contents with line numbering, offset, and limit safeguards. Reads supported images (png, jpeg, gif, webp, bmp) and attaches them to the result.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<ReadArgs>()
    }

    async fn call(&self, _context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        into_rig_result(self.execute(args).await)
    }
}
