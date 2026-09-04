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
        let home = config_dir.and_then(|_| resolve_home_dir());
        Self::discover_with_dirs(
            dir,
            ContextDirs {
                config_dir,
                home_dir: home.as_deref(),
                ..Default::default()
            },
        )
        .await
    }

    pub async fn discover_with_config(dir: impl AsRef<Path>, config: &rho_harness_core::config::Config) -> Self {
        let home = resolve_home_dir();
        Self::discover_with_dirs(
            dir,
            ContextDirs {
                config_dir: Some(&config.config_dir),
                home_dir: home.as_deref(),
                system_prompt: config.system_prompt.as_deref(),
                append_system_prompt: config.append_system_prompt.as_deref(),
                no_context_files: config.no_context_files,
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

        let mut base_system_prompt = resolve_base_system_prompt(base, dirs);
        if let Some(append) = dirs.append_system_prompt {
            let trimmed = append.trim();
            if !trimmed.is_empty() {
                if !base_system_prompt.is_empty() {
                    base_system_prompt = format!("{}\n\n{}", base_system_prompt.trim(), trimmed);
                } else {
                    base_system_prompt = trimmed.to_string();
                }
            }
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

fn resolve_home_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

fn resolve_base_system_prompt(base: &Path, dirs: ContextDirs<'_>) -> String {
    if let Some(prompt) = dirs.system_prompt {
        return prompt.to_string();
    }
    let mut prompt = DEFAULT_SYSTEM_PROMPT.to_string();
    if let Some(home) = dirs.home_dir
        && let Ok(custom) = std::fs::read_to_string(home.join(".agents/SYSTEM.md"))
    {
        prompt = custom;
    }
    if let Some(cfg) = dirs.config_dir
        && let Ok(custom) = std::fs::read_to_string(cfg.join("SYSTEM.md"))
    {
        prompt = custom;
    }
    for candidate in [".agents/SYSTEM.md", ".rho/SYSTEM.md", "prompts/SYSTEM.md", "SYSTEM.md"] {
        if let Ok(custom) = std::fs::read_to_string(base.join(candidate)) {
            return custom;
        }
    }
    prompt
}
