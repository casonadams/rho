pub mod normalize;
#[cfg(test)]
mod tests;

use crate::tools::atomic::atomic_write;
use crate::tools::engine_tool::EngineTool;
use crate::tools::types::{ToolResult, generated_schema};
pub use normalize::{detect_line_ending, has_whitespace_relaxed_match, normalize_line_endings, truncate_snippet};
pub use rho_harness_core::args::EditArgs;
pub use rho_harness_core::args::EditReplacement;
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::Workspace;
use std::path::{Path, PathBuf};

pub struct EditTool {
    pub base_dir: PathBuf,
}

async fn read_edit_file(path: &Path, clean_path: &str, base: &Path) -> std::result::Result<String, ToolResult> {
    let Ok(metadata) = tokio::fs::metadata(path).await else {
        return Err(ToolResult::error(format!(
            "File not found for edit: {clean_path} (in working directory: {})",
            base.display()
        )));
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

fn match_error(content: &str, old_text: &str, i: usize, count: usize) -> ToolResult {
    if count == 0 {
        let hint = if has_whitespace_relaxed_match(content, old_text) {
            "\n\nNote: A matching block with different whitespace or indentation was found. Verify exact indentation and line breaks."
        } else {
            ""
        };
        ToolResult::error(format!(
            "Edit #{}: oldText not found in file (exact match required):\n{}{hint}",
            i + 1,
            truncate_snippet(old_text, 120)
        ))
    } else {
        ToolResult::error(format!(
            "Edit #{}: oldText found {count} times in file (must be unique):\n{}\n\nNote: Provide more surrounding context lines in oldText to disambiguate the match.",
            i + 1,
            truncate_snippet(old_text, 120)
        ))
    }
}

fn apply_single_replacement(
    current_content: &str,
    edit: &EditReplacement,
    i: usize,
    line_ending: &str,
) -> std::result::Result<(String, usize), ToolResult> {
    let normalized_old = normalize_line_endings(&edit.old_text, line_ending);
    let normalized_new = normalize_line_endings(&edit.new_text, line_ending);
    if normalized_old.is_empty() {
        return Err(ToolResult::error(format!("Edit #{}: oldText must not be empty", i + 1)));
    }
    let mut indices = current_content.match_indices(normalized_old.as_ref());
    let first = indices.next();
    let second = indices.next();
    match (first, second) {
        (None, _) => Err(match_error(current_content, &edit.old_text, i, 0)),
        (Some(_), Some(_)) => {
            let count = 2 + indices.count();
            Err(match_error(current_content, &edit.old_text, i, count))
        }
        (Some((match_idx, _)), None) => {
            let line_num = 1 + current_content[..match_idx].matches('\n').count();
            let mut updated = String::with_capacity(
                current_content.len() + normalized_new.len().saturating_sub(normalized_old.len()),
            );
            updated.push_str(&current_content[..match_idx]);
            updated.push_str(normalized_new.as_ref());
            updated.push_str(&current_content[match_idx + normalized_old.len()..]);
            Ok((updated, line_num))
        }
    }
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

async fn write_edit_result(
    path: &Path,
    clean_path: &str,
    content: String,
    line_numbers: Vec<usize>,
    count: usize,
) -> Result<ToolResult, AppError> {
    match atomic_write(path, content.as_bytes()).await {
        Ok(_) => Ok(ToolResult {
            content: format!("Successfully applied {count} replacement(s) to {clean_path}"),
            is_error: false,
            metadata: Some(serde_json::json!({ "line_numbers": line_numbers })),
            image: None,
        }),
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

    pub fn with_exclusions<I, P>(base_dir: impl AsRef<Path>, _exclusions: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        Self::new(base_dir)
    }

    fn validate_edit_target(&self, clean_path: &str) -> std::result::Result<(Workspace, PathBuf), ToolResult> {
        if clean_path.is_empty() {
            return Err(ToolResult::error("Empty file path provided for edit tool"));
        }
        let workspace = Workspace::new(&self.base_dir);
        let Some(path) = workspace.resolve(clean_path) else {
            return Err(ToolResult::error("Empty file path provided for edit tool"));
        };
        Ok((workspace, path))
    }

    pub async fn execute(&self, args: EditArgs) -> Result<ToolResult, AppError> {
        let clean_path = args.path.trim().trim_matches('"').trim_matches('\'');
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
        write_edit_result(&path, clean_path, updated, lines, args.edits.len()).await
    }
}

#[async_trait::async_trait]
impl EngineTool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Edit a file by applying exact string replacements. Every oldText must match exactly once."
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<EditArgs>()
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, AppError> {
        let args: EditArgs = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::error(format!("failed to parse tool arguments: {e}"))),
        };
        self.execute(args).await
    }
}
