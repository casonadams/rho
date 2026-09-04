use super::*;

#[tokio::test]
async fn test_project_context_discovery() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_test_{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();
    tokio::fs::write(temp_dir.join("AGENTS.md"), "# Agent Rules\nBe concise.\n")
        .await
        .unwrap();

    let skills_dir = temp_dir.join("skills").join("plan");
    tokio::fs::create_dir_all(&skills_dir).await.unwrap();
    tokio::fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: plan\ndescription: Plan before code\n---\n# Plan skill\n",
    )
    .await
    .unwrap();

    let ctx = ProjectContext::discover(&temp_dir, None).await;
    assert_eq!(ctx.instruction_files.len(), 1);
    assert!(ctx.instruction_files[0].0.ends_with("AGENTS.md"));
    assert_eq!(ctx.skills.len(), 1);
    assert!(ctx.skills.iter().any(|s| s.name == "plan"));

    let prompt = ctx.build_system_prompt();
    assert!(prompt.contains("Agent Rules"));
    assert!(prompt.contains("<available_skills>"));
    assert!(prompt.contains("<name>plan</name>"));
    assert!(prompt.contains("Plan before code"));
    assert!(prompt.contains("Available tools"));
    assert!(prompt.contains("Today's date is"));
    assert!(prompt.contains("Platform:"));
    assert!(prompt.contains("Use read to examine files instead of cat or sed"));
    assert!(prompt.contains("Inspect the repository before asking"));

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_user_config_skills_discovery() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_override_test_{}", uuid::Uuid::new_v4()));
    let home_dir = temp_dir.join("home");
    let config_dir = temp_dir.join("config");
    let project_dir = temp_dir.join("project");
    let user_skill_dir = home_dir.join(".agents").join("skills").join("plan");
    let ignored_skill_dir = config_dir.join("skills").join("ignored");

    tokio::fs::create_dir_all(&user_skill_dir).await.unwrap();
    tokio::fs::create_dir_all(&ignored_skill_dir).await.unwrap();
    tokio::fs::create_dir_all(&project_dir).await.unwrap();

    tokio::fs::write(
        user_skill_dir.join("SKILL.md"),
        "---\nname: plan\ndescription: Custom user plan override\n---\n# Custom Plan\n",
    )
    .await
    .unwrap();
    tokio::fs::write(
        ignored_skill_dir.join("SKILL.md"),
        "---\nname: ignored\ndescription: Ignored config skill\n---\n# Ignored\n",
    )
    .await
    .unwrap();

    let ctx = ProjectContext::discover_with_dirs(
        &project_dir,
        ContextDirs {
            config_dir: Some(&config_dir),
            home_dir: Some(&home_dir),
        },
    )
    .await;
    let plan_skill = ctx.skills.iter().find(|s| s.name == "plan").unwrap();
    assert_eq!(plan_skill.description, "Custom user plan override");
    assert!(plan_skill.location.contains(".agents/skills/plan/SKILL.md"));
    assert!(!ctx.skills.iter().any(|s| s.name == "ignored"));

    let prompt = ctx.build_system_prompt();
    assert!(prompt.contains("Custom user plan override"));
    assert!(prompt.contains(".agents/skills/plan/SKILL.md"));

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_global_agents_md_discovery_hierarchy() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_hierarchy_test_{}", uuid::Uuid::new_v4()));
    let home_dir = temp_dir.join("home");
    let config_dir = temp_dir.join("config");
    let project_dir = temp_dir.join("project");

    let global_agents_dir = home_dir.join(".agents");
    let xdg_agents_dir = home_dir.join(".config").join("agents");
    let project_agents_dir = project_dir.join(".agents");

    tokio::fs::create_dir_all(&global_agents_dir).await.unwrap();
    tokio::fs::create_dir_all(&xdg_agents_dir).await.unwrap();
    tokio::fs::create_dir_all(&config_dir).await.unwrap();
    tokio::fs::create_dir_all(&project_agents_dir).await.unwrap();

    tokio::fs::write(global_agents_dir.join("AGENTS.md"), "# 1. Global User Rules\n")
        .await
        .unwrap();
    tokio::fs::write(xdg_agents_dir.join("AGENTS.md"), "# 2. XDG Global Rules\n")
        .await
        .unwrap();
    tokio::fs::write(config_dir.join("AGENTS.md"), "# 3. Rho Config Rules\n")
        .await
        .unwrap();
    tokio::fs::write(project_agents_dir.join("AGENTS.md"), "# 4. Project Base Rules\n")
        .await
        .unwrap();
    tokio::fs::write(project_dir.join("AGENTS.md"), "# 5. Project Active Rules\n")
        .await
        .unwrap();

    let ctx = ProjectContext::discover_with_dirs(
        &project_dir,
        ContextDirs {
            config_dir: Some(&config_dir),
            home_dir: Some(&home_dir),
        },
    )
    .await;

    // Only HOME/.agents and project directories are searched
    assert_eq!(ctx.instruction_files.len(), 3);
    assert_eq!(ctx.instruction_files[0].1, "# 1. Global User Rules");
    assert_eq!(ctx.instruction_files[1].1, "# 4. Project Base Rules");
    assert_eq!(ctx.instruction_files[2].1, "# 5. Project Active Rules");

    let prompt = ctx.build_system_prompt();
    let idx1 = prompt.find("# 1. Global User Rules").unwrap();
    let idx4 = prompt.find("# 4. Project Base Rules").unwrap();
    let idx5 = prompt.find("# 5. Project Active Rules").unwrap();

    assert!(idx1 < idx4);
    assert!(idx4 < idx5);
    assert!(!prompt.contains("# 2. XDG Global Rules"));
    assert!(!prompt.contains("# 3. Rho Config Rules"));

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_instruction_deduplication_via_symlink() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_dedup_test_{}", uuid::Uuid::new_v4()));
    let home_dir = temp_dir.join("home");
    let config_dir = temp_dir.join("config");
    let project_dir = temp_dir.join("project");
    let project_agents_dir = project_dir.join(".agents");

    tokio::fs::create_dir_all(&project_agents_dir).await.unwrap();

    let canonical_file = project_agents_dir.join("AGENTS.md");
    tokio::fs::write(&canonical_file, "# Canonical Rules\n").await.unwrap();

    #[cfg(unix)]
    std::os::unix::fs::symlink(&canonical_file, project_dir.join("AGENTS.md")).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&canonical_file, project_dir.join("AGENTS.md")).unwrap();

    let ctx = ProjectContext::discover_with_dirs(
        &project_dir,
        ContextDirs {
            config_dir: Some(&config_dir),
            home_dir: Some(&home_dir),
        },
    )
    .await;

    // The file should only be loaded once despite existing at both paths
    assert_eq!(ctx.instruction_files.len(), 1);
    assert_eq!(ctx.instruction_files[0].1, "# Canonical Rules");

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}
