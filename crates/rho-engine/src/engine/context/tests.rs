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
    assert!(ctx.skills.len() >= 2);
    assert!(ctx.skills.iter().any(|s| s.name == "plan"));
    assert!(ctx.skills.iter().any(|s| s.name == "create-plugin"));

    let prompt = ctx.build_system_prompt();
    assert!(prompt.contains("Agent Rules"));
    assert!(prompt.contains("<available_skills>"));
    assert!(prompt.contains("<name>plan</name>"));
    assert!(prompt.contains("Plan before code"));
    assert!(prompt.contains("Available tools"));
    assert!(prompt.contains("Today's date:"));
    assert!(prompt.contains("Platform:"));
    assert!(prompt.contains("Use read to examine files instead of cat or sed"));
    assert!(prompt.contains("Inspect the repository before asking"));
    assert!(prompt.contains("one ask_user_question call"));

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_user_config_skills_override_builtin_skills() {
    let temp_dir = std::env::temp_dir().join(format!("ctx_override_test_{}", uuid::Uuid::new_v4()));
    let config_dir = temp_dir.join("config");
    let project_dir = temp_dir.join("project");
    let user_skill_dir = config_dir.join("skills").join("plan");

    tokio::fs::create_dir_all(&user_skill_dir).await.unwrap();
    tokio::fs::create_dir_all(&project_dir).await.unwrap();

    tokio::fs::write(
        user_skill_dir.join("SKILL.md"),
        "---\nname: plan\ndescription: Custom user plan override\n---\n# Custom Plan\n",
    )
    .await
    .unwrap();

    let ctx = ProjectContext::discover(&project_dir, Some(&config_dir)).await;
    let plan_skill = ctx.skills.iter().find(|s| s.name == "plan").unwrap();
    assert_eq!(plan_skill.description, "Custom user plan override");
    assert!(plan_skill.location.contains("config/skills/plan/SKILL.md"));

    let prompt = ctx.build_system_prompt();
    assert!(prompt.contains("Custom user plan override"));
    assert!(prompt.contains("config/skills/plan/SKILL.md"));

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}
