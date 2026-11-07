pub mod template;

use std::collections::BTreeMap;
use std::path::Path;

pub use template::{PromptTemplate, PromptTemplateMetadata};

pub static DEFAULT_SYSTEM_PROMPT: &str = include_str!("SYSTEM.md");

fn is_distinct_project_dir(cwd: &Path) -> bool {
    cwd != crate::config::default_config_dir() && cwd != crate::config::dirs_fallback()
}

async fn load_project_prompts(cwd: &Path, target: &mut BTreeMap<String, PromptTemplate>) {
    let project_prompts = cwd.join("prompts");
    load_templates_from_dir(&project_prompts, "project", target).await;
    if is_distinct_project_dir(cwd) {
        let project_dot_claude = cwd.join(".claude").join("prompts");
        load_templates_from_dir(&project_dot_claude, "project", target).await;
        let project_dot_agents = cwd.join(".agents").join("prompts");
        load_templates_from_dir(&project_dot_agents, "project", target).await;
    }
}

async fn load_user_prompts(
    home_dir: Option<&Path>,
    config_dir: Option<&Path>,
    target: &mut BTreeMap<String, PromptTemplate>,
) {
    if let Some(cfg) = config_dir {
        load_templates_from_dir(&cfg.join("prompts"), "user", target).await;
    }
    if let Some(home) = home_dir {
        load_templates_from_dir(&home.join(".claude").join("prompts"), "user", target).await;
        load_templates_from_dir(&home.join(".agents").join("prompts"), "user", target).await;
    }
}

pub async fn discover_prompt_templates_for_paths_async(
    home_dir: Option<&Path>,
    config_dir: Option<&Path>,
    cwd: Option<&Path>,
) -> Vec<PromptTemplate> {
    let mut resolved = BTreeMap::new();
    load_user_prompts(home_dir, config_dir, &mut resolved).await;
    if let Some(cwd) = cwd {
        load_project_prompts(cwd, &mut resolved).await;
    }
    resolved.into_values().collect()
}

pub async fn discover_prompt_templates_async(config_dir: Option<&Path>, cwd: Option<&Path>) -> Vec<PromptTemplate> {
    let home = crate::config::dirs_fallback();
    discover_prompt_templates_for_paths_async(Some(&home), config_dir, cwd).await
}

async fn load_templates_from_dir(dir: &Path, origin: &str, target: &mut BTreeMap<String, PromptTemplate>) {
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let is_file = entry.metadata().await.map(|m| m.is_file()).unwrap_or(false);
        if !is_file || path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
            continue;
        };
        let Ok(content) = tokio::fs::read_to_string(&path).await else {
            continue;
        };
        let template = PromptTemplate::parse(&stem, &content, origin);
        target.insert(stem, template);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_discover_prompt_templates_precedence() {
        let temp_dir = std::env::temp_dir().join(format!("prompts_test_{}", uuid::Uuid::new_v4()));
        let user_config = temp_dir.join("user_config").join("prompts");
        let user_home = temp_dir.join("home");
        let user_agents = user_home.join(".agents").join("prompts");
        let project_dir = temp_dir.join("project");
        let project_prompts = project_dir.join("prompts");
        let project_agents = project_dir.join(".agents").join("prompts");
        let legacy_rho = project_dir.join(".rho").join("prompts");

        std::fs::create_dir_all(&user_config).unwrap();
        std::fs::create_dir_all(&user_agents).unwrap();
        std::fs::create_dir_all(&project_prompts).unwrap();
        std::fs::create_dir_all(&project_agents).unwrap();
        std::fs::create_dir_all(&legacy_rho).unwrap();

        std::fs::write(
            user_config.join("review.md"),
            "---\ndescription: User config review\n---\nUser config review",
        )
        .unwrap();
        std::fs::write(user_config.join("useronly.md"), "User only").unwrap();

        std::fs::write(
            user_agents.join("review.md"),
            "---\ndescription: User agents review\n---\nUser agents review",
        )
        .unwrap();
        std::fs::write(user_agents.join("homeonly.md"), "Home only").unwrap();

        std::fs::write(
            project_prompts.join("review.md"),
            "---\ndescription: Project prompts review\n---\nProject prompts review",
        )
        .unwrap();
        std::fs::write(project_prompts.join("projonly.md"), "Project only").unwrap();

        std::fs::write(
            project_agents.join("review.md"),
            "---\ndescription: Project agents review\n---\nProject agents review",
        )
        .unwrap();
        std::fs::write(project_agents.join("agentsonly.md"), "Agents only").unwrap();

        std::fs::write(legacy_rho.join("legacy.md"), "Legacy rho").unwrap();

        let discovered = discover_prompt_templates_for_paths_async(
            Some(&user_home),
            Some(&temp_dir.join("user_config")),
            Some(&project_dir),
        )
        .await;

        assert_eq!(discovered.len(), 5);
        assert!(discovered.iter().all(|t| t.metadata.name != "legacy"));

        let review_tmpl = discovered.iter().find(|t| t.metadata.name == "review").unwrap();
        assert_eq!(review_tmpl.origin, "project");
        assert_eq!(
            review_tmpl.metadata.description,
            Some("Project agents review".to_string())
        );

        let useronly_tmpl = discovered.iter().find(|t| t.metadata.name == "useronly").unwrap();
        assert_eq!(useronly_tmpl.origin, "user");

        let homeonly_tmpl = discovered.iter().find(|t| t.metadata.name == "homeonly").unwrap();
        assert_eq!(homeonly_tmpl.origin, "user");

        let projonly_tmpl = discovered.iter().find(|t| t.metadata.name == "projonly").unwrap();
        assert_eq!(projonly_tmpl.origin, "project");

        let agentsonly_tmpl = discovered.iter().find(|t| t.metadata.name == "agentsonly").unwrap();
        assert_eq!(agentsonly_tmpl.origin, "project");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_ignores_legacy_rho_prompts() {
        let temp_dir = std::env::temp_dir().join(format!("prompts_legacy_{}", uuid::Uuid::new_v4()));
        let project_dir = temp_dir.join("project");
        let legacy_rho = project_dir.join(".rho").join("prompts");
        std::fs::create_dir_all(&legacy_rho).unwrap();
        std::fs::write(legacy_rho.join("review.md"), "Legacy").unwrap();

        let discovered = discover_prompt_templates_for_paths_async(
            Some(&temp_dir.join("home")),
            Some(&temp_dir.join("user_config")),
            Some(&project_dir),
        )
        .await;
        assert!(discovered.is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn non_markdown_directories_and_unreadable_files_are_skipped() {
        let temp_dir = std::env::temp_dir().join(format!("prompts_skip_{}", uuid::Uuid::new_v4()));
        let user_config = temp_dir.join("user_config");
        let prompts = user_config.join("prompts");
        std::fs::create_dir_all(prompts.join("folder.md")).unwrap();
        std::fs::write(prompts.join("good.md"), "Good").unwrap();
        std::fs::write(prompts.join("notes.txt"), "Notes").unwrap();
        std::fs::write(prompts.join("binary.md"), [0xff_u8, 0xfe]).unwrap();

        let discovered =
            discover_prompt_templates_for_paths_async(Some(&temp_dir.join("home")), Some(&user_config), None).await;
        let names: Vec<&str> = discovered.iter().map(|t| t.metadata.name.as_str()).collect();
        assert_eq!(names, vec!["good"]);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
