use rho_harness_core::skills::SkillMetadata;
use std::path::{Path, PathBuf};

mod instructions;
mod prompt;
#[cfg(test)]
mod tests;

pub use instructions::ContextDirs;
pub use prompt::escape_xml;
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
    pub async fn discover(dir: impl AsRef<Path>, config_dir: Option<&Path>) -> Self {
        let home = if config_dir.is_some() {
            std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .ok()
                .map(PathBuf::from)
        } else {
            None
        };
        Self::discover_with_dirs(
            dir,
            ContextDirs {
                config_dir,
                home_dir: home.as_deref(),
            },
        )
        .await
    }

    pub async fn discover_with_dirs(dir: impl AsRef<Path>, dirs: ContextDirs<'_>) -> Self {
        let base = dir.as_ref();
        let instruction_files = instructions::discover_instructions(base, dirs);

        let paths = rho_harness_core::skills::SkillResolutionPaths {
            project_dir: Some(base),
            home_dir: dirs.home_dir,
        };
        let skills: Vec<SkillMetadata> = rho_harness_core::skills::resolved_skills_for_paths(paths)
            .into_iter()
            .map(|skill| skill.metadata)
            .collect();

        let mut base_system_prompt = DEFAULT_SYSTEM_PROMPT.to_string();
        if let Some(home) = dirs.home_dir
            && let Ok(custom) = std::fs::read_to_string(home.join(".agents/SYSTEM.md"))
        {
            base_system_prompt = custom;
        }
        if let Some(cfg) = dirs.config_dir
            && let Ok(custom) = std::fs::read_to_string(cfg.join("SYSTEM.md"))
        {
            base_system_prompt = custom;
        }
        if let Ok(custom) = std::fs::read_to_string(base.join(".agents/SYSTEM.md")) {
            base_system_prompt = custom;
        } else if let Ok(custom) = std::fs::read_to_string(base.join(".rho/SYSTEM.md")) {
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

    pub fn build_system_prompt(&self) -> String {
        prompt::build_system_prompt(self)
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
