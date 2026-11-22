pub mod matching;
pub mod normalize;
pub mod preview;
#[cfg(test)]
mod tests;

use crate::tools::atomic::atomic_write;
use crate::tools::engine_tool::TypedTool;
use crate::tools::types::{ToolResult, resolve_tool_path};
pub use matching::locate_edit;
pub use normalize::{detect_line_ending, has_whitespace_relaxed_match, normalize_line_endings, truncate_snippet};
pub use rho_harness_core::args::EditArgs;
pub use rho_harness_core::args::EditReplacement;
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::{Workspace, clean_path_arg};
use std::path::{Path, PathBuf};

pub struct EditTool {
    pub base_dir: PathBuf,
}

async fn read_edit_file(path: &Path, clean_path: &str, base: &Path) -> std::result::Result<String, ToolResult> {
    let metadata = match tokio::fs::metadata(path).await {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let hint =
                crate::tools::suggest::missing_path_hint(base, clean_path, crate::tools::suggest::PathKind::File).await;
            return Err(ToolResult::error(format!(
                "File not found for edit: {clean_path} (in working directory: {}){hint}",
                base.display()
            )));
        }
        Err(e) => return Err(ToolResult::error(format!("Failed to access {clean_path}: {e}"))),
    };
    if metadata.is_dir() {
        return Err(ToolResult::error(format!(
            "Cannot edit {clean_path}: target path is a directory"
        )));
    }
    tokio::fs::read_to_string(path)
        .await
        .map_err(|e| ToolResult::error(format!("Failed to read {clean_path}: {e}")))
}

fn apply_single_replacement(
    current_content: &str,
    edit: &EditReplacement,
    i: usize,
    line_ending: &str,
) -> std::result::Result<(String, usize), ToolResult> {
    let ranges = locate_edit(current_content, edit, line_ending)
        .map_err(|message| ToolResult::error(format!("Edit #{}: {message}", i + 1)))?;
    let new_text = normalize_line_endings(&edit.new_text, line_ending);
    let line_num = 1 + current_content[..ranges[0].start].matches('\n').count();
    let mut updated = String::with_capacity(current_content.len() + new_text.len() * ranges.len());
    let mut cursor = 0;
    for range in ranges {
        updated.push_str(&current_content[cursor..range.start]);
        updated.push_str(new_text.as_ref());
        cursor = range.end;
    }
    updated.push_str(&current_content[cursor..]);
    Ok((updated, line_num))
}

fn apply_all_edits(content: &str, edits: &[EditReplacement]) -> std::result::Result<(String, Vec<usize>), ToolResult> {
    let line_ending = detect_line_ending(content);
    let mut current = content.to_string();
    let mut line_numbers = Vec::with_capacity(edits.len());
    for (i, edit) in edits.iter().enumerate() {
        let (updated, line_num) = apply_single_replacement(&current, edit, i, line_ending)?;
        current = updated;
        line_numbers.push(line_num);
    }
    Ok((current, line_numbers))
}

struct EditOutcome<'a> {
    clean_path: &'a str,
    before: &'a str,
    after: String,
    line_numbers: Vec<usize>,
    count: usize,
}

async fn write_edit_result(path: &Path, outcome: EditOutcome<'_>) -> Result<ToolResult, AppError> {
    let EditOutcome {
        clean_path,
        before,
        after,
        line_numbers,
        count,
    } = outcome;
    match atomic_write(path, after.as_bytes()).await {
        Ok(_) => {
            let regions = preview::changed_regions(before, &after);
            let mut message = format!("Successfully applied {count} replacement(s) to {clean_path}");
            if !regions.is_empty() {
                message.push('\n');
                message.push_str(&regions);
            }
            Ok(ToolResult::success(message).with_metadata(serde_json::json!({ "line_numbers": line_numbers })))
        }
        Err(e) => Ok(ToolResult::error(format!(
            "Failed to write updated file {clean_path}: {e}"
        ))),
    }
}

impl EditTool {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    fn validate_edit_target(&self, clean_path: &str) -> std::result::Result<(Workspace, PathBuf), ToolResult> {
        let workspace = Workspace::new(&self.base_dir);
        let path = resolve_tool_path(&workspace, clean_path, "edit")?;
        Ok((workspace, path))
    }

    pub async fn execute(&self, args: EditArgs) -> Result<ToolResult, AppError> {
        let clean_path = clean_path_arg(&args.path);
        let (workspace, path) = match self.validate_edit_target(clean_path) {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let content = match read_edit_file(&path, clean_path, workspace.root()).await {
            Ok(c) => c,
            Err(e) => return Ok(e),
        };
        if args.edits.is_empty() {
            return Ok(ToolResult::error("No edits provided in edit tool call"));
        }
        let (updated, lines) = match apply_all_edits(&content, &args.edits) {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        write_edit_result(
            &path,
            EditOutcome {
                clean_path,
                before: &content,
                after: updated,
                line_numbers: lines,
                count: args.edits.len(),
            },
        )
        .await
    }
}

#[async_trait::async_trait]
impl TypedTool for EditTool {
    type Args = EditArgs;
    const NAME: &'static str = "edit";

    async fn run(&self, args: EditArgs) -> Result<ToolResult, AppError> {
        self.execute(args).await
    }
}
