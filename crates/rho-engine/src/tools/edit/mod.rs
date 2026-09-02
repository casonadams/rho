use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
pub use rho_harness_core::args::EditArgs;
pub use rho_harness_core::args::EditReplacement;
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::Workspace;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

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

        if args.edits.is_empty() {
            return Ok(ToolResult::error("No edits provided in edit tool call"));
        }

        let content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::error(format!("Failed to read {clean_path}: {e}"))),
        };

        // Validate all edits before applying
        let mut current_content = content.clone();
        for (i, edit) in args.edits.iter().enumerate() {
            if edit.old_text.is_empty() {
                return Ok(ToolResult::error(format!("Edit #{}: oldText must not be empty", i + 1)));
            }

            let matches: Vec<_> = current_content.match_indices(&edit.old_text).collect();
            if matches.is_empty() {
                return Ok(ToolResult::error(format!(
                    "Edit #{}: oldText not found in file (exact match required):\n{}",
                    i + 1,
                    truncate_snippet(&edit.old_text, 120)
                )));
            }
            if matches.len() > 1 {
                return Ok(ToolResult::error(format!(
                    "Edit #{}: oldText found {} times in file (must be unique):\n{}",
                    i + 1,
                    matches.len(),
                    truncate_snippet(&edit.old_text, 120)
                )));
            }

            current_content = current_content.replacen(&edit.old_text, &edit.new_text, 1);
        }

        // Revalidate after reading and validating all replacements, immediately before mutation.
        if !workspace.can_mutate(clean_path) {
            return Ok(ToolResult::error(format!(
                "Edit target moved outside the permitted workspace: {clean_path}"
            )));
        }

        // Commit atomically
        match tokio::fs::write(&path, &current_content).await {
            Ok(_) => Ok(ToolResult::success(format!(
                "Successfully applied {} replacement(s) to {}",
                args.edits.len(),
                clean_path
            ))),
            Err(e) => Ok(ToolResult::error(format!(
                "Failed to write updated file {clean_path}: {e}"
            ))),
        }
    }
}

fn truncate_snippet(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{truncated}...")
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
