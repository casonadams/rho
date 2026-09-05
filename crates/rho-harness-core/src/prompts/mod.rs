pub mod template;

use std::collections::BTreeMap;
use std::path::Path;

pub use template::{PromptTemplate, PromptTemplateMetadata};

pub static DEFAULT_SYSTEM_PROMPT: &str = include_str!("SYSTEM.md");

pub fn discover_prompt_templates(config_dir: Option<&Path>, cwd: Option<&Path>) -> Vec<PromptTemplate> {
    let mut resolved: BTreeMap<String, PromptTemplate> = BTreeMap::new();

    if let Some(config_dir) = config_dir {
        let user_prompts_dir = config_dir.join("prompts");
        load_templates_from_dir(&user_prompts_dir, "user", &mut resolved);
    }

    if let Some(cwd) = cwd {
        if cwd != crate::config::default_config_dir() && cwd != crate::config::dirs_fallback() {
            let project_dot_rho = cwd.join(".rho").join("prompts");
            load_templates_from_dir(&project_dot_rho, "project", &mut resolved);
        }

        let project_prompts = cwd.join("prompts");
        load_templates_from_dir(&project_prompts, "project", &mut resolved);
    }

    resolved.into_values().collect()
}

pub async fn discover_prompt_templates_async(config_dir: Option<&Path>, cwd: Option<&Path>) -> Vec<PromptTemplate> {
    let mut resolved = BTreeMap::new();

    if let Some(cfg) = config_dir {
        let user_prompts = cfg.join("prompts");
        load_templates_from_dir_async(&user_prompts, "user", &mut resolved).await;
    }

    if let Some(cwd) = cwd {
        if cwd != crate::config::default_config_dir() && cwd != crate::config::dirs_fallback() {
            let project_dot_rho = cwd.join(".rho/prompts");
            load_templates_from_dir_async(&project_dot_rho, "project", &mut resolved).await;
        }

        let project_prompts = cwd.join("prompts");
        load_templates_from_dir_async(&project_prompts, "project", &mut resolved).await;
    }

    resolved.into_values().collect()
}

fn load_templates_from_dir(dir: &Path, origin: &str, target: &mut BTreeMap<String, PromptTemplate>) {
    if !dir.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path.extension().and_then(|e| e.to_str()) == Some("md")
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            && let Ok(content) = std::fs::read_to_string(&path)
        {
            let template = PromptTemplate::parse(stem, &content, origin);
            target.insert(stem.to_string(), template);
        }
    }
}

async fn load_templates_from_dir_async(dir: &Path, origin: &str, target: &mut BTreeMap<String, PromptTemplate>) {
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md")
            && let Ok(metadata) = entry.metadata().await
            && metadata.is_file()
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            && let Ok(content) = tokio::fs::read_to_string(&path).await
        {
            let template = PromptTemplate::parse(stem, &content, origin);
            target.insert(stem.to_string(), template);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discover_prompt_templates_precedence() {
        let temp_dir = std::env::temp_dir().join(format!("prompts_test_{}", uuid::Uuid::new_v4()));
        let user_dir = temp_dir.join("user_config").join("prompts");
        let project_dir = temp_dir.join("project").join(".rho").join("prompts");

        std::fs::create_dir_all(&user_dir).unwrap();
        std::fs::create_dir_all(&project_dir).unwrap();

        std::fs::write(
            user_dir.join("review.md"),
            "---\ndescription: User review\n---\nUser review",
        )
        .unwrap();
        std::fs::write(user_dir.join("useronly.md"), "User only").unwrap();
        std::fs::write(
            project_dir.join("review.md"),
            "---\ndescription: Project review\n---\nProject review",
        )
        .unwrap();

        let discovered =
            discover_prompt_templates(Some(&temp_dir.join("user_config")), Some(&temp_dir.join("project")));

        assert_eq!(discovered.len(), 2);
        let review_tmpl = discovered.iter().find(|t| t.metadata.name == "review").unwrap();
        assert_eq!(review_tmpl.origin, "project");
        assert_eq!(review_tmpl.metadata.description, Some("Project review".to_string()));

        let useronly_tmpl = discovered.iter().find(|t| t.metadata.name == "useronly").unwrap();
        assert_eq!(useronly_tmpl.origin, "user");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
