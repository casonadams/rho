use super::super::*;

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
            ..Default::default()
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
            ..Default::default()
        },
    )
    .await;

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
            ..Default::default()
        },
    )
    .await;

    assert_eq!(ctx.instruction_files.len(), 1);
    assert_eq!(ctx.instruction_files[0].1, "# Canonical Rules");

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_project_context_discovery_with_transclusion() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_trans_test_{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();

    let docs_dir = temp_dir.join("docs");
    tokio::fs::create_dir_all(&docs_dir).await.unwrap();
    tokio::fs::write(docs_dir.join("standards.md"), "Inlined development standards.\n")
        .await
        .unwrap();

    tokio::fs::write(
        temp_dir.join("AGENTS.md"),
        "# Root Rules\n@docs/standards.md\nAlways test.\n",
    )
    .await
    .unwrap();

    let ctx = ProjectContext::discover(&temp_dir, None).await;
    assert_eq!(ctx.instruction_files.len(), 1);
    assert!(ctx.instruction_files[0].1.contains("Inlined development standards."));
    assert!(ctx.instruction_files[0].1.contains("Always test."));

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_repository_ancestry_walk_up_ordering() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_ancestry_test_{}", uuid::Uuid::new_v4()));
    let repo_root = temp_dir.join("repo");
    let crates_dir = repo_root.join("crates");
    let engine_dir = crates_dir.join("engine");

    tokio::fs::create_dir_all(repo_root.join(".git")).await.unwrap();
    tokio::fs::create_dir_all(&engine_dir).await.unwrap();

    tokio::fs::write(repo_root.join("AGENTS.md"), "# 1. Root Workspace Rules\n")
        .await
        .unwrap();
    tokio::fs::write(crates_dir.join("AGENTS.md"), "# 2. Crates Intermediate Rules\n")
        .await
        .unwrap();
    tokio::fs::write(engine_dir.join("AGENTS.md"), "# 3. Engine Subtree Rules\n")
        .await
        .unwrap();

    let ctx = ProjectContext::discover_with_dirs(&engine_dir, ContextDirs::default()).await;

    assert_eq!(ctx.instruction_files.len(), 3);
    assert_eq!(ctx.instruction_files[0].1, "# 1. Root Workspace Rules");
    assert_eq!(ctx.instruction_files[1].1, "# 2. Crates Intermediate Rules");
    assert_eq!(ctx.instruction_files[2].1, "# 3. Engine Subtree Rules");

    let prompt = ctx.build_system_prompt();
    let idx1 = prompt.find("# 1. Root Workspace Rules").unwrap();
    let idx2 = prompt.find("# 2. Crates Intermediate Rules").unwrap();
    let idx3 = prompt.find("# 3. Engine Subtree Rules").unwrap();
    assert!(idx1 < idx2);
    assert!(idx2 < idx3);

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_find_repo_root_and_ancestry_helpers() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_repo_root_test_{}", uuid::Uuid::new_v4()));
    let repo_root = temp_dir.join("repo");
    let sub_dir = repo_root.join("a").join("b").join("c");
    let non_repo = temp_dir.join("other");

    tokio::fs::create_dir_all(repo_root.join(".git")).await.unwrap();
    tokio::fs::create_dir_all(&sub_dir).await.unwrap();
    tokio::fs::create_dir_all(&non_repo).await.unwrap();

    assert_eq!(find_repo_root(&sub_dir), Some(repo_root.clone()));
    assert_eq!(find_repo_root(&repo_root), Some(repo_root.clone()));
    assert!(find_repo_root(&non_repo).is_none());

    tokio::fs::write(repo_root.join("AGENTS.md"), "# Repo Root\n")
        .await
        .unwrap();
    tokio::fs::write(sub_dir.join("AGENTS.md"), "# Sub Leaf\n")
        .await
        .unwrap();

    let discovered = discover_ancestry_instructions(&sub_dir, Some(&repo_root));
    assert_eq!(discovered.len(), 2);
    assert_eq!(discovered[0].1, "# Repo Root");
    assert_eq!(discovered[1].1, "# Sub Leaf");

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_ancestry_walk_up_with_global_and_transclusion() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_global_trans_ancestry_{}", uuid::Uuid::new_v4()));
    let home_dir = temp_dir.join("home");
    let repo_root = temp_dir.join("repo");
    let leaf_dir = repo_root.join("crates").join("engine");
    let docs_dir = repo_root.join("docs");

    tokio::fs::create_dir_all(home_dir.join(".agents")).await.unwrap();
    tokio::fs::create_dir_all(repo_root.join(".git")).await.unwrap();
    tokio::fs::create_dir_all(&leaf_dir).await.unwrap();
    tokio::fs::create_dir_all(&docs_dir).await.unwrap();

    tokio::fs::write(home_dir.join(".agents").join("AGENTS.md"), "# Global Rules\n")
        .await
        .unwrap();
    tokio::fs::write(docs_dir.join("standards.md"), "Inlined engineering standards.\n")
        .await
        .unwrap();
    tokio::fs::write(repo_root.join("AGENTS.md"), "# Root Rules\n@docs/standards.md\n")
        .await
        .unwrap();
    tokio::fs::write(leaf_dir.join("AGENTS.md"), "# Leaf Engine Rules\n")
        .await
        .unwrap();

    let ctx = ProjectContext::discover_with_dirs(
        &leaf_dir,
        ContextDirs {
            home_dir: Some(&home_dir),
            ..Default::default()
        },
    )
    .await;

    assert_eq!(ctx.instruction_files.len(), 3);
    assert_eq!(ctx.instruction_files[0].1, "# Global Rules");
    assert!(ctx.instruction_files[1].1.contains("Inlined engineering standards."));
    assert_eq!(ctx.instruction_files[2].1, "# Leaf Engine Rules");

    let prompt = ctx.build_system_prompt();
    let idx_global = prompt.find("# Global Rules").unwrap();
    let idx_standards = prompt.find("Inlined engineering standards.").unwrap();
    let idx_leaf = prompt.find("# Leaf Engine Rules").unwrap();
    assert!(idx_global < idx_standards);
    assert!(idx_standards < idx_leaf);

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}
