use crate::error::AppError;
use crate::tools::approval::enforce_approval;
use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
use crate::tools::workspace::Workspace;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct EditReplacement {
    /// Exact text in the file to replace (must match exactly once)
    #[serde(rename = "oldText")]
    pub old_text: String,
    /// Replacement text
    #[serde(rename = "newText")]
    pub new_text: String,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct EditArgs {
    /// Path to the file to edit (relative or absolute)
    pub path: String,
    /// List of exact replacements to apply
    pub edits: Vec<EditReplacement>,
}

pub struct EditTool {
    pub base_dir: PathBuf,
}

impl EditTool {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    pub async fn execute(&self, args: EditArgs) -> Result<ToolResult, AppError> {
        let clean_path = args.path.trim().trim_matches('"').trim_matches('\'');
        if clean_path.is_empty() {
            return Ok(ToolResult::error("Empty file path provided for edit tool"));
        }

        let workspace = Workspace::new(&self.base_dir);
        let Some(path) = workspace.resolve(clean_path) else {
            return Ok(ToolResult::error("Empty file path provided for edit tool"));
        };
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

    async fn call(&self, context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        enforce_approval(context, Self::NAME, &args)?;
        into_rig_result(self.execute(args).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_edit_unique_replacement() {
        let temp_dir = std::env::temp_dir().join(format!("edit_test_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let file_path = temp_dir.join("sample.txt");
        tokio::fs::write(&file_path, "fn hello() {\n    println!(\"world\");\n}\n")
            .await
            .unwrap();

        let tool = EditTool::new(&temp_dir);
        let res = tool
            .execute(EditArgs {
                path: file_path.to_str().unwrap().to_string(),
                edits: vec![EditReplacement {
                    old_text: "println!(\"world\");".to_string(),
                    new_text: "println!(\"rust\");".to_string(),
                }],
            })
            .await
            .unwrap();

        assert!(!res.is_error);
        let updated = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(updated, "fn hello() {\n    println!(\"rust\");\n}\n");

        let _ = tokio::fs::remove_dir_all(temp_dir).await;
    }

    #[tokio::test]
    async fn test_edit_duplicate_match_fails_atomically() {
        let temp_dir = std::env::temp_dir().join(format!("edit_test_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let file_path = temp_dir.join("sample.txt");
        let initial_content = "foo bar foo baz\n";
        tokio::fs::write(&file_path, initial_content).await.unwrap();

        let tool = EditTool::new(&temp_dir);
        let res = tool
            .execute(EditArgs {
                path: file_path.to_str().unwrap().to_string(),
                edits: vec![EditReplacement {
                    old_text: "foo".to_string(),
                    new_text: "qux".to_string(),
                }],
            })
            .await
            .unwrap();

        assert!(res.is_error);
        assert!(res.content.contains("found 2 times"));
        let disk = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(disk, initial_content); // Unchanged

        let _ = tokio::fs::remove_dir_all(temp_dir).await;
    }

    #[tokio::test]
    async fn test_edit_missing_match_fails_atomically() {
        let temp_dir = std::env::temp_dir().join(format!("edit_test_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let file_path = temp_dir.join("sample.txt");
        let initial_content = "hello world\n";
        tokio::fs::write(&file_path, initial_content).await.unwrap();

        let tool = EditTool::new(&temp_dir);
        let res = tool
            .execute(EditArgs {
                path: file_path.to_str().unwrap().to_string(),
                edits: vec![EditReplacement {
                    old_text: "not_present".to_string(),
                    new_text: "replacement".to_string(),
                }],
            })
            .await
            .unwrap();

        assert!(res.is_error);
        assert!(res.content.contains("oldText not found"));
        let disk = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(disk, initial_content);

        let _ = tokio::fs::remove_dir_all(temp_dir).await;
    }
}
