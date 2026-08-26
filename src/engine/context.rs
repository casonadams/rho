use crate::intent::model::IntentSpec;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ProjectContext {
    pub current_dir: PathBuf,
    pub instruction_files: Vec<(String, String)>,
    pub git_status: Option<String>,
    pub os_info: String,
    pub date_str: String,
}

impl ProjectContext {
    pub async fn discover(dir: impl AsRef<Path>) -> Self {
        let base = dir.as_ref();
        let mut instruction_files = Vec::new();

        let candidates = ["AGENTS.md", "CLAUDE.md", ".cursorrules"];

        for filename in candidates {
            let file_path = base.join(filename);
            if file_path.exists()
                && let Ok(content) = tokio::fs::read_to_string(&file_path).await
            {
                instruction_files.push((filename.to_string(), content.trim().to_string()));
            }
        }

        let git_status = get_git_summary(base).await;
        let os_info = format!("{} ({})", std::env::consts::OS, std::env::consts::ARCH);
        let date_str = chrono::Local::now().format("%A, %B %d, %Y, %H:%M:%S %Z").to_string();

        Self {
            current_dir: base.to_path_buf(),
            instruction_files,
            git_status,
            os_info,
            date_str,
        }
    }

    pub fn build_system_prompt(&self, intent: Option<&IntentSpec>) -> String {
        let mut prompt = String::new();
        prompt.push_str("You are an expert coding assistant operating inside rust-ai, a coding agent harness. You help users by reading files, executing commands, editing code, and writing new files.\n\n");

        prompt.push_str(&format!("Today's date: {}\n", self.date_str));
        prompt.push_str(&format!("Platform: {}\n\n", self.os_info));

        prompt.push_str("Available tools:\n");
        prompt.push_str("- read: Read file contents (with offset/limit safeguards)\n");
        prompt.push_str("- write: Create or overwrite files (automatically creates parent directories)\n");
        prompt.push_str(
            "- edit: Make precise file edits with exact text replacement (every edits[].oldText must match uniquely)\n",
        );
        prompt.push_str("- bash: Execute bash commands (ls, rg, find, cargo, git, etc.)\n");
        prompt.push_str("- ask_user: Ask the user a question or present choices to clarify requirements or make implementation decisions\n");
        prompt.push_str("- websearch: Search the web and return structured summaries and URLs\n");
        prompt.push_str("- webfetch: Fetch and extract clean text or markdown from URLs\n\n");

        prompt.push_str("Guidelines:\n");
        prompt.push_str("- Use bash for file operations like ls, rg, find\n");
        prompt.push_str("- Commands run directly in the working directory; do not prefix commands with cd\n");
        prompt.push_str("- Use read to examine files instead of cat or sed\n");
        prompt.push_str("- Use edit for precise changes (edits[].oldText must match exactly)\n");
        prompt.push_str("- When changing multiple separate locations in one file, use one edit call with multiple entries in edits[] instead of multiple edit calls\n");
        prompt.push_str("- Keep edits[].oldText as small as possible while still being unique in the file\n");
        prompt.push_str("- Use write only for new files or complete rewrites\n");
        prompt.push_str(
            "- Treat IntentSpec outcomes and constraints as binding resolved decisions; do not ask for them again\n",
        );
        prompt
            .push_str("- Inspect the repository before asking about implementation details that the code can answer\n");
        prompt.push_str("- When unresolved user decisions block progress, ask them together in one ask_user call; treat the answers as binding IntentSpec additions\n");
        prompt.push_str("- Be concise in your responses\n");
        prompt.push_str("- Show file paths clearly when working with files\n");

        if let Some(ref git) = self.git_status {
            prompt.push_str(&format!("- Git repository status: {git}\n"));
        }
        prompt.push('\n');

        if !self.instruction_files.is_empty() {
            prompt.push_str("<project_context>\n\nProject-specific instructions and guidelines:\n\n");
            for (name, content) in &self.instruction_files {
                prompt.push_str(&format!(
                    "<project_instructions path=\"{name}\">\n{content}\n</project_instructions>\n\n"
                ));
            }
            prompt.push_str("</project_context>\n\n");
        }

        if let Some(spec) = intent {
            prompt.push_str(&spec.to_system_prompt_section());
            prompt.push('\n');
        }

        let clean_cwd = self.current_dir.display().to_string().replace('\\', "/");
        prompt.push_str(&format!("Current working directory: {clean_cwd}"));

        prompt
    }
}

async fn get_git_summary(dir: &Path) -> Option<String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("status").arg("--short").arg("--branch");
    cmd.current_dir(dir);
    let out = cmd.output().await.ok()?;
    if out.status.success() {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() {
            return Some(s.lines().take(5).collect::<Vec<_>>().join(" | "));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_project_context_discovery() {
        let temp_dir = std::env::temp_dir().join(format!("ctx_test_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        tokio::fs::write(temp_dir.join("AGENTS.md"), "# Agent Rules\nBe concise.\n")
            .await
            .unwrap();

        let ctx = ProjectContext::discover(&temp_dir).await;
        assert_eq!(ctx.instruction_files.len(), 1);
        assert_eq!(ctx.instruction_files[0].0, "AGENTS.md");

        let prompt = ctx.build_system_prompt(None);
        assert!(prompt.contains("Agent Rules"));
        assert!(prompt.contains("Available tools"));
        assert!(prompt.contains("Today's date:"));
        assert!(prompt.contains("Platform:"));
        assert!(prompt.contains("Use read to examine files instead of cat or sed"));
        assert!(prompt.contains("do not ask for them again"));
        assert!(prompt.contains("Inspect the repository before asking"));
        assert!(prompt.contains("one ask_user call"));

        let _ = tokio::fs::remove_dir_all(temp_dir).await;
    }
}
