use super::super::*;

fn setup_project_context(temp_dir: &std::path::Path) {
    std::fs::create_dir_all(temp_dir).unwrap();
    std::fs::write(temp_dir.join("AGENTS.md"), "# Agent Rules\nBe concise.\n").unwrap();
    let skills_dir = temp_dir.join("skills").join("plan");
    std::fs::create_dir_all(&skills_dir).unwrap();
    std::fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: plan\ndescription: Plan before code\n---\n# Plan skill\n",
    )
    .unwrap();
}

#[tokio::test]
async fn test_project_context_discovery() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_test_{}", uuid::Uuid::new_v4()));
    setup_project_context(&temp_dir);

    let ctx = ProjectContext::discover(&temp_dir, None).await;
    assert_eq!((ctx.instruction_files.len(), ctx.skills.len()), (1, 1));
    assert!(ctx.instruction_files[0].0.ends_with("AGENTS.md"));

    let prompt = ctx.build_system_prompt();
    let fragments = [
        "Agent Rules",
        "<available_skills>",
        "<name>plan</name>",
        "Plan before code",
        "Available tools",
        "Today's date is",
        "Platform:",
        "Use read to examine files",
    ];
    for f in fragments {
        assert!(prompt.contains(f));
    }
    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

fn setup_user_skills(home: &std::path::Path, config: &std::path::Path, proj: &std::path::Path) {
    let user_skill_dir = home.join(".agents").join("skills").join("plan");
    let ignored_skill_dir = config.join("skills").join("ignored");
    std::fs::create_dir_all(&user_skill_dir).unwrap();
    std::fs::create_dir_all(&ignored_skill_dir).unwrap();
    std::fs::create_dir_all(proj).unwrap();
    std::fs::write(
        user_skill_dir.join("SKILL.md"),
        "---\nname: plan\ndescription: Custom user plan override\n---\n# Custom Plan\n",
    )
    .unwrap();
    std::fs::write(
        ignored_skill_dir.join("SKILL.md"),
        "---\nname: ignored\ndescription: Ignored\n---\n# Ignored\n",
    )
    .unwrap();
}

#[tokio::test]
async fn test_user_config_skills_discovery() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_override_test_{}", uuid::Uuid::new_v4()));
    let (home_dir, config_dir, project_dir) =
        (temp_dir.join("home"), temp_dir.join("config"), temp_dir.join("project"));
    setup_user_skills(&home_dir, &config_dir, &project_dir);

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
    assert!(!ctx.skills.iter().any(|s| s.name == "ignored"));

    let prompt = ctx.build_system_prompt();
    assert!(prompt.contains("Custom user plan override"));
    assert!(prompt.contains(".agents/skills/plan/SKILL.md"));
    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

fn setup_hierarchy_agents_files(home: &std::path::Path, cfg: &std::path::Path, proj: &std::path::Path) {
    for dir in [
        &home.join(".agents"),
        &home.join(".config/agents"),
        cfg,
        &proj.join(".agents"),
        proj,
    ] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(home.join(".agents/AGENTS.md"), "# 1. Global User Rules\n").unwrap();
    std::fs::write(home.join(".config/agents/AGENTS.md"), "# 2. XDG Global Rules\n").unwrap();
    std::fs::write(cfg.join("AGENTS.md"), "# 3. Rho Config Rules\n").unwrap();
    std::fs::write(proj.join(".agents/AGENTS.md"), "# 4. Project Base Rules\n").unwrap();
    std::fs::write(proj.join("AGENTS.md"), "# 5. Project Active Rules\n").unwrap();
}

#[tokio::test]
async fn test_global_agents_md_discovery_hierarchy() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_hierarchy_test_{}", uuid::Uuid::new_v4()));
    let (home_dir, config_dir, project_dir) =
        (temp_dir.join("home"), temp_dir.join("config"), temp_dir.join("project"));
    setup_hierarchy_agents_files(&home_dir, &config_dir, &project_dir);

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
    let (repo_root, crates_dir) = (temp_dir.join("repo"), temp_dir.join("repo/crates"));
    let engine_dir = crates_dir.join("engine");

    std::fs::create_dir_all(repo_root.join(".git")).unwrap();
    std::fs::create_dir_all(&engine_dir).unwrap();
    std::fs::write(repo_root.join("AGENTS.md"), "# 1. Root Workspace Rules\n").unwrap();
    std::fs::write(crates_dir.join("AGENTS.md"), "# 2. Crates Intermediate Rules\n").unwrap();
    std::fs::write(engine_dir.join("AGENTS.md"), "# 3. Engine Subtree Rules\n").unwrap();

    let ctx = ProjectContext::discover_with_dirs(&engine_dir, ContextDirs::default()).await;
    assert_eq!(ctx.instruction_files.len(), 3);
    for i in 1..=3 {
        assert!(ctx.instruction_files[i - 1].1.contains(&format!("# {i}.")));
    }
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
    let (home_dir, repo_root) = (temp_dir.join("home"), temp_dir.join("repo"));
    let (leaf_dir, docs_dir) = (repo_root.join("crates/engine"), repo_root.join("docs"));

    for dir in [&home_dir.join(".agents"), &repo_root.join(".git"), &leaf_dir, &docs_dir] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(home_dir.join(".agents/AGENTS.md"), "# Global Rules\n").unwrap();
    std::fs::write(docs_dir.join("standards.md"), "Inlined engineering standards.\n").unwrap();
    std::fs::write(repo_root.join("AGENTS.md"), "# Root Rules\n@docs/standards.md\n").unwrap();
    std::fs::write(leaf_dir.join("AGENTS.md"), "# Leaf Engine Rules\n").unwrap();

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
    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}
