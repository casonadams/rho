use rho_harness_core::skills::SkillMetadata;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

pub use rho_harness_core::prompts::DEFAULT_SYSTEM_PROMPT;

#[derive(Debug, Clone)]
pub struct ProjectContext {
    pub current_dir: PathBuf,
    pub base_system_prompt: String,
    pub instruction_files: Vec<(String, String)>,
    pub skills: Vec<SkillMetadata>,
    pub git_status: Option<String>,
    pub os_info: String,
    pub date_str: String,
}

impl ProjectContext {
    pub async fn discover(dir: impl AsRef<Path>, config_dir: Option<&Path>, include_builtins: bool) -> Self {
        let base = dir.as_ref();
        let mut instruction_files = Vec::new();

        if let Some(cfg) = config_dir {
            Self::load_candidate_instructions(cfg, &mut instruction_files);
        }
        Self::load_candidate_instructions(base, &mut instruction_files);

        let skills: Vec<SkillMetadata> =
            rho_harness_core::skills::resolved_skills(config_dir, Some(base), include_builtins)
                .into_iter()
                .map(|skill| skill.metadata)
                .collect();

        let mut base_system_prompt = DEFAULT_SYSTEM_PROMPT.to_string();
        if let Some(cfg) = config_dir
            && let Ok(custom) = std::fs::read_to_string(cfg.join("SYSTEM.md"))
        {
            base_system_prompt = custom;
        }
        if let Ok(custom) = std::fs::read_to_string(base.join(".rho/SYSTEM.md")) {
            base_system_prompt = custom;
        } else if let Ok(custom) = std::fs::read_to_string(base.join("prompts/SYSTEM.md")) {
            base_system_prompt = custom;
        } else if let Ok(custom) = std::fs::read_to_string(base.join("SYSTEM.md")) {
            base_system_prompt = custom;
        }

        let git_status = get_git_summary(base).await;
        let os_info = format!("{} ({})", std::env::consts::OS, std::env::consts::ARCH);
        let date_str = chrono::Local::now().format("%Y-%m-%d").to_string();

        Self {
            current_dir: base.to_path_buf(),
            base_system_prompt,
            instruction_files,
            skills,
            git_status,
            os_info,
            date_str,
        }
    }

    /// Re-read only the per-turn volatile fields; files and skill metadata are
    /// cached by the caller for the lifetime of the working directory.
    pub async fn refresh_runtime_state(&mut self) {
        self.git_status = get_git_summary(&self.current_dir).await;
        self.date_str = chrono::Local::now().format("%Y-%m-%d").to_string();
    }

    fn load_candidate_instructions(dir: &Path, files: &mut Vec<(String, String)>) {
        let candidates = ["AGENTS.md", "CLAUDE.md", ".cursorrules"];
        for filename in candidates {
            let file_path = dir.join(filename);
            if file_path.exists()
                && let Ok(content) = std::fs::read_to_string(&file_path)
            {
                let path_display = file_path.display().to_string();
                if !files.iter().any(|(p, _)| p == &path_display) {
                    files.push((path_display, content.trim().to_string()));
                }
            }
        }
    }

    pub fn build_system_prompt(&self) -> String {
        let mut prompt = String::new();
        prompt.push_str(self.base_system_prompt.trim());
        prompt.push_str("\n\n");

        if !self.instruction_files.is_empty() {
            prompt.push_str("<project_context>\n\nProject-specific instructions and guidelines:\n\n");
            for (name, content) in &self.instruction_files {
                prompt.push_str(&format!(
                    "<project_instructions path=\"{}\">\n{}\n</project_instructions>\n\n",
                    escape_xml(name),
                    content
                ));
            }
            prompt.push_str("</project_context>\n\n");
        }

        if !self.skills.is_empty() {
            prompt.push_str("The following skills provide specialized instructions for specific tasks.\n");
            prompt.push_str("Use the read tool to load a skill's file when the task matches its description.\n");
            prompt.push_str("When a skill file references a relative path, resolve it against the skill directory (parent of SKILL.md / dirname of the path) and use that absolute path in tool commands.\n\n");
            prompt.push_str("<available_skills>\n");
            for skill in &self.skills {
                prompt.push_str("  <skill>\n");
                prompt.push_str(&format!("    <name>{}</name>\n", escape_xml(&skill.name)));
                prompt.push_str(&format!(
                    "    <description>{}</description>\n",
                    escape_xml(&skill.description)
                ));
                prompt.push_str(&format!("    <location>{}</location>\n", escape_xml(&skill.location)));
                prompt.push_str("  </skill>\n");
            }
            prompt.push_str("</available_skills>\n\n");
        }

        let clean_cwd = self.current_dir.display().to_string().replace('\\', "/");
        prompt.push_str(&format!("Current working directory: {clean_cwd}\n\n"));
        prompt.push_str(&format!(
            "Today's date is {}. When searching for recent events, releases, or \"latest\" information, factor in this current date.\n",
            self.date_str
        ));
        prompt.push_str(&format!("Platform: {}", self.os_info));

        if let Some(ref git) = self.git_status {
            prompt.push_str(&format!("\nGit repository status: {git}"));
        }

        prompt
    }
}

pub fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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
