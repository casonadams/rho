use super::{collected_output, collecting_renderer};
use crate::config::Config;
use crate::repl::commands::{CommandResult, SlashCommandContext, SlashCommandHandler};
use rho_engine::auth::AuthStore;

fn write_test_skill(workspace: &std::path::Path, name: &str, body: &str) {
    let dir = workspace.join(".agents").join("skills").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("SKILL.md"), body).unwrap();
}

fn skill_context<'a>(
    (config, auth): (&'a mut Config, &'a mut AuthStore),
    renderer: &'a crate::ui::TerminalRenderer,
    home_dir: Option<&'a std::path::Path>,
) -> SlashCommandContext<'a> {
    SlashCommandContext {
        config,
        auth_store: auth,
        renderer,
        session_id: None,
        session_manager: None,
        engine: None,
        home_dir,
    }
}

#[tokio::test]
async fn skill_command_lists_resolved_overrides_with_origin() {
    let workspace = std::env::temp_dir().join(format!("skill_cmd_{}", uuid::Uuid::new_v4()));
    write_test_skill(
        &workspace,
        "team-notes",
        "---\nname: team-notes\ndescription: User notes workflow\n---\n# Notes\nnever executed\n",
    );

    let (mut config, mut auth) = (Config::default(), AuthStore::default());
    let (renderer, mut events) = collecting_renderer();
    let mut context = skill_context((&mut config, &mut auth), &renderer, Some(&workspace));

    let listing = SlashCommandHandler::handle("/skills", &mut context).await.unwrap();
    assert!(matches!(listing, Some(CommandResult::Continue)));
    assert!(collected_output(&mut events).contains("    - team-notes: User notes workflow (user)"));

    let viewing = SlashCommandHandler::handle("/skill team-notes", &mut context)
        .await
        .unwrap();
    assert!(matches!(viewing, Some(CommandResult::Continue)));
    let viewed = collected_output(&mut events);
    assert!(viewed.contains("[skill: team-notes (user)]") && viewed.contains("# Notes"));

    let _ = std::fs::remove_dir_all(&workspace);
}

#[tokio::test]
async fn skill_command_reports_unknown_names_with_available_skills() {
    let (mut config, mut auth) = (Config::default(), AuthStore::default());
    let (renderer, mut events) = collecting_renderer();
    let mut context = skill_context((&mut config, &mut auth), &renderer, None);

    let result = SlashCommandHandler::handle("/skill does-not-exist", &mut context)
        .await
        .unwrap();

    assert!(matches!(result, Some(CommandResult::Continue)));
    let output = collected_output(&mut events);
    assert!(output.contains("does-not-exist") && output.contains("Available skills"));
}

#[tokio::test]
async fn test_slash_skill_colon_invocation() {
    let workspace = std::env::temp_dir().join(format!("skill_colon_{}", uuid::Uuid::new_v4()));
    write_test_skill(
        &workspace,
        "my-flow",
        "---\nname: my-flow\ndescription: Custom flow\n---\nRun step A then step B",
    );

    let (mut config, mut auth) = (Config::default(), AuthStore::default());
    let (renderer, _) = collecting_renderer();
    let mut context = skill_context((&mut config, &mut auth), &renderer, Some(&workspace));

    let result = SlashCommandHandler::handle("/skill:my-flow create foo", &mut context)
        .await
        .unwrap();
    let Some(CommandResult::ExpandedPrompt { text }) = result else {
        panic!("expected ExpandedPrompt");
    };
    assert!(text.contains("Run step A then step B") && text.contains("Skill input: create foo"));
    let _ = std::fs::remove_dir_all(workspace);
}
