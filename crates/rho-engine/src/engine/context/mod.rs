use rho_harness_core::skills::SkillMetadata;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub mod activation;
mod instructions;
mod prompt;
#[cfg(test)]
mod tests;
pub mod transclusion;

pub use activation::{MAX_DYNAMIC_INSTRUCTION_BYTES, MAX_DYNAMIC_INSTRUCTION_FILES};
pub use instructions::{
    ContextDirs, discover_ancestry_instructions, discover_instructions, discover_instructions_with_seen, find_repo_root,
};
pub use prompt::escape_xml;
pub use rho_harness_core::prompts::DEFAULT_SYSTEM_PROMPT;
pub use transclusion::expand_transclusions;

#[derive(Debug, Clone)]
pub struct ProjectContext {
    pub current_dir: PathBuf,
    pub base_system_prompt: String,
    pub instruction_files: Vec<(String, String)>,
    pub skills: Vec<SkillMetadata>,
    pub git_status: Option<String>,
    pub os_info: String,
    pub date_str: String,
    pub seen_instruction_files: HashSet<PathBuf>,
    pub no_context_files: bool,
    pub dynamic_instructions_count: usize,
    pub dynamic_instructions_bytes: usize,
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
        let (instruction_files, seen_instruction_files) =
            instructions::discover_instructions_with_seen_async(base, dirs).await;

        let base_owned = base.to_path_buf();
        let home_owned = dirs.home_dir.map(Path::to_path_buf);
        let skills: Vec<SkillMetadata> = tokio::task::spawn_blocking(move || {
            let paths = rho_harness_core::skills::SkillResolutionPaths {
                project_dir: Some(&base_owned),
                home_dir: home_owned.as_deref(),
            };
            rho_harness_core::skills::resolved_skills_for_paths(paths)
                .into_iter()
                .map(|skill| skill.metadata)
                .collect()
        })
        .await
        .unwrap_or_default();

        let mut base_system_prompt = resolve_base_system_prompt(base, dirs).await;
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
            seen_instruction_files,
            no_context_files: dirs.no_context_files,
            dynamic_instructions_count: 0,
            dynamic_instructions_bytes: 0,
        }
    }

    pub fn activate_path_instructions(&mut self, path: &Path) {
        activation::activate_path_instructions(self, path);
    }

    pub async fn activate_path_instructions_async(&mut self, path: &Path) {
        activation::activate_path_instructions_async(self, path).await;
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

async fn resolve_base_system_prompt(base: &Path, dirs: ContextDirs<'_>) -> String {
    if let Some(prompt) = dirs.system_prompt {
        return prompt.to_string();
    }
    let mut prompt = DEFAULT_SYSTEM_PROMPT.to_string();
    if let Some(home) = dirs.home_dir
        && let Ok(custom) = tokio::fs::read_to_string(home.join(".agents/SYSTEM.md")).await
    {
        prompt = custom;
    }
    if let Some(cfg) = dirs.config_dir
        && let Ok(custom) = tokio::fs::read_to_string(cfg.join("SYSTEM.md")).await
    {
        prompt = custom;
    }
    for candidate in [".agents/SYSTEM.md", ".rho/SYSTEM.md", "prompts/SYSTEM.md", "SYSTEM.md"] {
        if let Ok(custom) = tokio::fs::read_to_string(base.join(candidate)).await {
            return custom;
        }
    }
    prompt
}
