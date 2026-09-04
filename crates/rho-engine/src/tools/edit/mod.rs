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

    pub async fn execute(&self, args: EditArgs) -> Result<ToolResult, AppError> {
        let clean_path = args.path.trim().trim_matches('"').trim_matches('\'');
        if clean_path.is_empty() {
            return Ok(ToolResult::error("Empty file path provided for edit tool"));
        }

        let workspace = Workspace::with_exclusions(&self.base_dir, &self.exclusions);
        let Some(path) = workspace.resolve(clean_path) else {
            return Ok(ToolResult::error("Empty file path provided for edit tool"));
        };
        if !workspace.can_mutate(clean_path) {
            return Ok(ToolResult::error(format!(
                "Edit target is outside the permitted workspace: {clean_path}"
            )));
        }
        let base = workspace.root();

        if !path.exists() {
            return Ok(ToolResult::error(format!(
                "File not found for edit: {} (in working directory: {})",
                clean_path,
                base.display()
            )));
        }

        if path.is_dir() {
            return Ok(ToolResult::error(format!(
                "Cannot edit {clean_path}: target path is a directory"
            )));
        }

        if args.edits.is_empty() {
            return Ok(ToolResult::error("No edits provided in edit tool call"));
        }

        let content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::error(format!("Failed to read {clean_path}: {e}"))),
        };

        let line_ending = detect_line_ending(&content);
        let mut current_content = content.clone();

        let mut line_numbers = Vec::new();

        for (i, edit) in args.edits.iter().enumerate() {
            let normalized_old = normalize_line_endings(&edit.old_text, line_ending);
            let normalized_new = normalize_line_endings(&edit.new_text, line_ending);

            if normalized_old.is_empty() {
                return Ok(ToolResult::error(format!("Edit #{}: oldText must not be empty", i + 1)));
            }

            let matches: Vec<_> = current_content.match_indices(normalized_old.as_ref()).collect();
            if matches.is_empty() {
                let hint = if has_whitespace_relaxed_match(&current_content, &normalized_old) {
                    "\n\nNote: A matching block with different whitespace or indentation was found. Verify exact indentation and line breaks."
                } else {
                    ""
                };
                return Ok(ToolResult::error(format!(
                    "Edit #{}: oldText not found in file (exact match required):\n{}{hint}",
                    i + 1,
                    truncate_snippet(&edit.old_text, 120)
                )));
            }
            if matches.len() > 1 {
                return Ok(ToolResult::error(format!(
                    "Edit #{}: oldText found {} times in file (must be unique):\n{}\n\nNote: Provide more surrounding context lines in oldText to disambiguate the match.",
                    i + 1,
                    matches.len(),
                    truncate_snippet(&edit.old_text, 120)
                )));
            }

            let line_num = 1 + current_content[..matches[0].0].matches('\n').count();
            line_numbers.push(line_num);

            current_content = current_content.replacen(normalized_old.as_ref(), normalized_new.as_ref(), 1);
        }

        if !workspace.can_mutate(clean_path) {
            return Ok(ToolResult::error(format!(
                "Edit target moved outside the permitted workspace: {clean_path}"
            )));
        }

        match atomic_write(&path, current_content.as_bytes()).await {
            Ok(_) => Ok(ToolResult {
                content: format!(
                    "Successfully applied {} replacement(s) to {}",
                    args.edits.len(),
                    clean_path
                ),
                is_error: false,
                metadata: Some(serde_json::json!({
                    "line_numbers": line_numbers,
                })),
                image: None,
            }),
            Err(e) => Ok(ToolResult::error(format!(
                "Failed to write updated file {clean_path}: {e}"
            ))),
        }
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
