use super::{collected_output, collecting_renderer};
use crate::config::Config;
use crate::repl::commands::{CommandResult, SlashCommandContext, SlashCommandHandler};
use rho_engine::auth::AuthStore;

#[tokio::test]
async fn skill_command_lists_resolved_overrides_with_origin() {
    let workspace = std::env::temp_dir().join(format!("skill_cmd_{}", uuid::Uuid::new_v4()));
    let user_skill_dir = workspace.join(".agents").join("skills").join("team-notes");
    std::fs::create_dir_all(&user_skill_dir).unwrap();
    std::fs::write(
        user_skill_dir.join("SKILL.md"),
        "---\nname: team-notes\ndescription: User notes workflow\n---\n# Notes\nnever executed\n",
    )
    .unwrap();

    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, mut events) = collecting_renderer();
    let mut context = SlashCommandContext {
        config: &mut config,
        auth_store: &mut auth,
        renderer: &renderer,
        session_id: None,
        session_manager: None,
        engine: None,
        home_dir: Some(&workspace),
    };

    let listing = SlashCommandHandler::handle("/skills", &mut context).await.unwrap();
    assert!(matches!(listing, Some(CommandResult::Continue)));
    let output = collected_output(&mut events);
    assert!(
        output.contains("    - team-notes: User notes workflow (user)"),
        "{output}"
    );

    let viewing = SlashCommandHandler::handle("/skill team-notes", &mut context)
        .await
        .unwrap();
    assert!(matches!(viewing, Some(CommandResult::Continue)));
    let viewed = collected_output(&mut events);
    assert!(viewed.contains("[skill: team-notes (user)]"));
    assert!(viewed.contains("# Notes"));
    assert!(viewed.contains("never executed"));

    let _ = std::fs::remove_dir_all(&workspace);
}

#[tokio::test]
async fn skill_command_reports_unknown_names_with_available_skills() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, mut events) = collecting_renderer();
    let mut context = SlashCommandContext {
        config: &mut config,
        auth_store: &mut auth,
        renderer: &renderer,
        session_id: None,
        session_manager: None,
        engine: None,
        home_dir: None,
    };

    let result = SlashCommandHandler::handle("/skill does-not-exist", &mut context)
        .await
        .unwrap();

    assert!(matches!(result, Some(CommandResult::Continue)));
    let output = collected_output(&mut events);
    assert!(output.contains("does-not-exist"));
    assert!(output.contains("Available skills"));
}

#[tokio::test]
async fn test_slash_skill_colon_invocation() {
    let workspace = std::env::temp_dir().join(format!("skill_colon_{}", uuid::Uuid::new_v4()));
    let user_skill_dir = workspace.join(".agents").join("skills").join("my-flow");
    std::fs::create_dir_all(&user_skill_dir).unwrap();
    std::fs::write(
        user_skill_dir.join("SKILL.md"),
        "---\nname: my-flow\ndescription: Custom flow\n---\nRun step A then step B",
    )
    .unwrap();

    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, _) = collecting_renderer();
    let mut context = SlashCommandContext {
        config: &mut config,
        auth_store: &mut auth,
        renderer: &renderer,
        session_id: None,
        session_manager: None,
        engine: None,
        home_dir: Some(&workspace),
    };

    let result = SlashCommandHandler::handle("/skill:my-flow create foo", &mut context)
        .await
        .unwrap();
    assert!(
        matches!(result, Some(CommandResult::ExpandedPrompt { text }) if text.contains("Run step A then step B") && text.contains("Skill input: create foo"))
    );

    let _ = std::fs::remove_dir_all(workspace);
}
