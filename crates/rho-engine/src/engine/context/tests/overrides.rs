use super::super::*;
use rho_harness_core::config::Config;

#[tokio::test]
async fn test_system_prompt_override_ignores_system_md() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_sys_override_{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();
    tokio::fs::write(
        temp_dir.join("SYSTEM.md"),
        "File-based base prompt that should be ignored",
    )
    .await
    .unwrap();

    let ctx = ProjectContext::discover_with_dirs(
        &temp_dir,
        ContextDirs {
            system_prompt: Some("Custom override persona"),
            ..Default::default()
        },
    )
    .await;

    assert_eq!(ctx.base_system_prompt, "Custom override persona");
    let prompt = ctx.build_system_prompt();
    assert!(prompt.contains("Custom override persona"));
    assert!(!prompt.contains("File-based base prompt"));

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_append_system_prompt() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_append_{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();

    let ctx = ProjectContext::discover_with_dirs(
        &temp_dir,
        ContextDirs {
            append_system_prompt: Some("Situational rule: operate in read-only mode."),
            ..Default::default()
        },
    )
    .await;

    let prompt = ctx.build_system_prompt();
    assert!(prompt.contains(DEFAULT_SYSTEM_PROMPT.trim()));
    assert!(prompt.contains("Situational rule: operate in read-only mode."));
    assert!(
        ctx.base_system_prompt
            .ends_with("Situational rule: operate in read-only mode.")
    );

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_system_prompt_override_and_append() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_sys_and_append_{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();

    let ctx = ProjectContext::discover_with_dirs(
        &temp_dir,
        ContextDirs {
            system_prompt: Some("Base persona"),
            append_system_prompt: Some("Appended rule"),
            ..Default::default()
        },
    )
    .await;

    assert_eq!(ctx.base_system_prompt, "Base persona\n\nAppended rule");
    let prompt = ctx.build_system_prompt();
    assert!(prompt.contains("Base persona\n\nAppended rule"));

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_no_context_files_suppression() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_no_ctx_{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();
    tokio::fs::write(temp_dir.join("AGENTS.md"), "# Project Instructions\n")
        .await
        .unwrap();
    tokio::fs::write(temp_dir.join("CLAUDE.md"), "# Claude Instructions\n")
        .await
        .unwrap();
    tokio::fs::write(temp_dir.join(".cursorrules"), "cursor rules\n")
        .await
        .unwrap();

    let skills_dir = temp_dir.join("skills").join("plan");
    tokio::fs::create_dir_all(&skills_dir).await.unwrap();
    tokio::fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: plan\ndescription: Plan before code\n---\n# Plan\n",
    )
    .await
    .unwrap();

    let ctx = ProjectContext::discover_with_dirs(
        &temp_dir,
        ContextDirs {
            no_context_files: true,
            ..Default::default()
        },
    )
    .await;

    assert!(ctx.instruction_files.is_empty());
    assert_eq!(ctx.skills.len(), 1);
    assert!(ctx.skills.iter().any(|s| s.name == "plan"));

    let prompt = ctx.build_system_prompt();
    assert!(!prompt.contains("<project_context>"));
    assert!(!prompt.contains("<project_instructions"));
    assert!(!prompt.contains("Project Instructions"));
    assert!(prompt.contains("<available_skills>"));

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_discover_with_config() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_cfg_{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();
    tokio::fs::write(temp_dir.join("AGENTS.md"), "# Suppressed Rules\n")
        .await
        .unwrap();

    let config = Config {
        system_prompt: Some("Config persona".to_string()),
        append_system_prompt: Some("Config addition".to_string()),
        no_context_files: true,
        ..Default::default()
    };

    let ctx = ProjectContext::discover_with_config(&temp_dir, &config).await;

    assert_eq!(ctx.base_system_prompt, "Config persona\n\nConfig addition");
    assert!(ctx.instruction_files.is_empty());

    let prompt = ctx.build_system_prompt();
    assert!(prompt.contains("Config persona\n\nConfig addition"));
    assert!(!prompt.contains("<project_instructions"));

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}
