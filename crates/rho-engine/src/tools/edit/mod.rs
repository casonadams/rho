pub mod normalize;
#[cfg(test)]
mod tests;

use crate::tools::atomic::atomic_write;
use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
pub use normalize::{detect_line_ending, has_whitespace_relaxed_match, normalize_line_endings, truncate_snippet};
pub use rho_harness_core::args::EditArgs;
pub use rho_harness_core::args::EditReplacement;
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::Workspace;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use std::path::{Path, PathBuf};

pub struct EditTool {
    pub base_dir: PathBuf,
    exclusions: Vec<PathBuf>,
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

fn match_error(content: &str, old_text: &str, (i, count): (usize, usize)) -> ToolResult {
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
    (i, line_ending): (usize, &str),
) -> std::result::Result<(String, usize), ToolResult> {
    let normalized_old = normalize_line_endings(&edit.old_text, line_ending);
    let normalized_new = normalize_line_endings(&edit.new_text, line_ending);
    if normalized_old.is_empty() {
        return Err(ToolResult::error(format!("Edit #{}: oldText must not be empty", i + 1)));
    }
    let matches: Vec<_> = current_content.match_indices(normalized_old.as_ref()).collect();
    if matches.len() != 1 {
        return Err(match_error(current_content, &edit.old_text, (i, matches.len())));
    }
    let line_num = 1 + current_content[..matches[0].0].matches('\n').count();
    let updated = current_content.replacen(normalized_old.as_ref(), normalized_new.as_ref(), 1);
    Ok((updated, line_num))
}

fn apply_all_edits(content: &str, edits: &[EditReplacement]) -> std::result::Result<(String, Vec<usize>), ToolResult> {
    let line_ending = detect_line_ending(content);
    let mut current = content.to_string();
    let mut line_numbers = Vec::with_capacity(edits.len());
    for (i, edit) in edits.iter().enumerate() {
        let (updated, line_num) = apply_single_replacement(&current, edit, (i, line_ending))?;
        current = updated;
        line_numbers.push(line_num);
    }
    Ok((current, line_numbers))
}

async fn write_edit_result(
    path: &Path,
    clean_path: &str,
    (content, line_numbers, count): (String, Vec<usize>, usize),
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

    fn validate_edit_target(&self, clean_path: &str) -> std::result::Result<(Workspace, PathBuf), ToolResult> {
        if clean_path.is_empty() {
            return Err(ToolResult::error("Empty file path provided for edit tool"));
        }
        let workspace = Workspace::with_exclusions(&self.base_dir, &self.exclusions);
        let Some(path) = workspace.resolve(clean_path) else {
            return Err(ToolResult::error("Empty file path provided for edit tool"));
        };
        if !workspace.can_mutate(clean_path) {
            return Err(ToolResult::error(format!(
                "Edit target is outside the permitted workspace: {clean_path}"
            )));
        }
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
        if !workspace.can_mutate(clean_path) {
            return Ok(ToolResult::error(format!(
                "Edit target moved outside the permitted workspace: {clean_path}"
            )));
        }
        write_edit_result(&path, clean_path, (updated, lines, args.edits.len())).await
    }
}

impl Tool for EditTool {
    const NAME: &'static str = "edit";
    type Args = EditArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Edit a file by applying exact string replacements. Every oldText must match exactly once.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<EditArgs>()
    }

    async fn call(&self, _context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        into_rig_result(self.execute(args).await)
    }
}
