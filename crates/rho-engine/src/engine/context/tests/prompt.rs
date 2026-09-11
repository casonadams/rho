use super::super::*;
use rho_harness_core::prompts::DEFAULT_SYSTEM_PROMPT;
use rho_harness_core::skills::SkillMetadata;

fn assert_contains_all(target: &str, expected: &[&str]) {
    for fragment in expected {
        assert!(target.contains(fragment));
    }
}

fn assert_contains_none(target: &str, unexpected: &[&str]) {
    for fragment in unexpected {
        assert!(!target.contains(fragment));
    }
}

#[test]
fn default_tools_assemble_matches_static_system_prompt() {
    let default_tools = default_tool_names();
    let assembled = assemble_base_system_prompt(&default_tools);
    assert_eq!(assembled.trim(), DEFAULT_SYSTEM_PROMPT.trim());
}

#[test]
fn dynamic_tools_omits_unregistered_tools() {
    let read_only_tools = vec!["read".to_string(), "fd".to_string()];
    let assembled = assemble_base_system_prompt(&read_only_tools);

    assert_contains_all(&assembled, &["- read:", "- fd:"]);
    assert_contains_none(&assembled, &["- write:", "- edit:", "- bash:"]);
}

#[test]
fn dynamic_tools_omits_unregistered_guidelines() {
    let read_only_tools = vec!["read".to_string(), "fd".to_string()];
    let assembled = assemble_base_system_prompt(&read_only_tools);

    assert_contains_all(
        &assembled,
        &[
            "Use read to examine files instead of cat or sed",
            "Use fd for file discovery",
        ],
    );
    assert_contains_none(
        &assembled,
        &[
            "Use edit for precise changes",
            "Use write only for new files",
            "Commands run directly in the working directory",
            "Never use bash for file inspection",
            "Use rg for content search",
        ],
    );
}

#[test]
fn bash_fallback_guideline_when_search_tools_absent() {
    let shell_tools = vec!["bash".to_string(), "read".to_string()];
    let assembled = assemble_base_system_prompt(&shell_tools);

    assert_contains_all(
        &assembled,
        &[
            "Use bash for file operations like ls, rg, find",
            "Commands run directly in the working directory",
        ],
    );
    assert_contains_none(&assembled, &["Use fd for file discovery"]);
}

#[test]
fn custom_or_unknown_tools_render_gracefully() {
    let custom_tools = vec!["custom_analyzer".to_string()];
    let assembled = assemble_base_system_prompt(&custom_tools);

    assert!(assembled.contains("- custom_analyzer\n"));
    assert!(assembled.contains("Be concise in your responses"));
}

#[test]
fn empty_tools_renders_none() {
    let empty_tools: Vec<String> = Vec::new();
    let assembled = assemble_base_system_prompt(&empty_tools);

    assert!(assembled.contains("(none)"));
}

fn sample_skills() -> Vec<SkillMetadata> {
    vec![
        SkillMetadata {
            name: "visible-skill".to_string(),
            description: "Visible skill description".to_string(),
            location: "/path/to/visible/SKILL.md".to_string(),
            disable_model_invocation: false,
        },
        SkillMetadata {
            name: "hidden-skill".to_string(),
            description: "Hidden skill description".to_string(),
            location: "/path/to/hidden/SKILL.md".to_string(),
            disable_model_invocation: true,
        },
    ]
}

#[tokio::test]
async fn skills_instruction_uses_read_tool_when_available() {
    let temp = std::env::temp_dir().join(format!("test_read_{}", uuid::Uuid::new_v4()));
    let _ = tokio::fs::create_dir_all(&temp).await;
    let mut ctx = ProjectContext::discover_with_dirs(
        &temp,
        ContextDirs {
            active_tools: Some(&["read".to_string()]),
            ..Default::default()
        },
    )
    .await;
    ctx.skills = sample_skills();

    let prompt = ctx.build_system_prompt();
    assert_contains_all(
        &prompt,
        &["<name>visible-skill</name>", "Use the read tool to load a skill's file"],
    );
    assert_contains_none(&prompt, &["<name>hidden-skill</name>"]);
    let _ = tokio::fs::remove_dir_all(temp).await;
}

#[tokio::test]
async fn skills_instruction_uses_bash_when_read_absent() {
    let temp = std::env::temp_dir().join(format!("test_bash_{}", uuid::Uuid::new_v4()));
    let _ = tokio::fs::create_dir_all(&temp).await;
    let mut ctx = ProjectContext::discover_with_dirs(
        &temp,
        ContextDirs {
            active_tools: Some(&["bash".to_string()]),
            ..Default::default()
        },
    )
    .await;
    ctx.skills = sample_skills();

    let prompt = ctx.build_system_prompt();
    assert_contains_all(
        &prompt,
        &["<name>visible-skill</name>", "Use bash to load a skill's file"],
    );
    assert_contains_none(&prompt, &["<name>hidden-skill</name>"]);
    let _ = tokio::fs::remove_dir_all(temp).await;
}

#[tokio::test]
async fn skills_omitted_when_no_reader_tool_active() {
    let temp = std::env::temp_dir().join(format!("test_none_{}", uuid::Uuid::new_v4()));
    let _ = tokio::fs::create_dir_all(&temp).await;
    let mut ctx = ProjectContext::discover_with_dirs(
        &temp,
        ContextDirs {
            active_tools: Some(&["write".to_string()]),
            ..Default::default()
        },
    )
    .await;
    ctx.skills = sample_skills();

    let prompt = ctx.build_system_prompt();
    assert_contains_none(&prompt, &["<available_skills>", "visible-skill"]);
    let _ = tokio::fs::remove_dir_all(temp).await;
}
