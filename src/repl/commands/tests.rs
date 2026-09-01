use super::*;
use crate::ui::interactive::{InteractiveUi, OutputEvent, UiEvent};
use tokio::sync::mpsc;

fn collecting_renderer() -> (TerminalRenderer, mpsc::UnboundedReceiver<UiEvent>) {
    let (ui, events) = InteractiveUi::channel();
    (TerminalRenderer::with_ui(ui), events)
}

fn collected_output(events: &mut mpsc::UnboundedReceiver<UiEvent>) -> String {
    std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event {
            UiEvent::Output(OutputEvent::Text(text)) => Some(text),
            UiEvent::Transcript(crate::ui::interactive::TranscriptItem::Notice(text)) => Some(text),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn skill_command_lists_resolved_overrides_with_origin() {
    let workspace = std::env::temp_dir().join(format!("skill_cmd_{}", uuid::Uuid::new_v4()));
    let config_dir = workspace.join("config");
    let user_skill_dir = config_dir.join("skills").join("team-notes");
    std::fs::create_dir_all(&user_skill_dir).unwrap();
    std::fs::write(
        user_skill_dir.join("SKILL.md"),
        "---\nname: team-notes\ndescription: User notes workflow\n---\n# Notes\nnever executed\n",
    )
    .unwrap();

    let mut config = Config {
        config_dir,
        ..Config::default()
    };
    let mut auth = AuthStore::default();
    let (renderer, mut events) = collecting_renderer();
    let mut context = SlashCommandContext {
        config: &mut config,
        auth_store: &mut auth,
        renderer: &renderer,
        session_id: None,
        session_manager: None,
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
async fn help_is_emitted_through_the_renderer() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, mut events) = collecting_renderer();
    let mut context = SlashCommandContext {
        config: &mut config,
        auth_store: &mut auth,
        renderer: &renderer,
        session_id: None,
        session_manager: None,
    };

    let result = SlashCommandHandler::handle("/help", &mut context).await.unwrap();

    assert!(matches!(result, Some(CommandResult::Continue)));
    let output = collected_output(&mut events);
    assert!(output.contains("/model [model] [provider]"));
    assert!(output.contains("Current session"));
}

#[tokio::test]
async fn login_is_dispatched_without_collecting_credentials() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, _) = collecting_renderer();
    let mut context = SlashCommandContext {
        config: &mut config,
        auth_store: &mut auth,
        renderer: &renderer,
        session_id: None,
        session_manager: None,
    };
    let result = SlashCommandHandler::handle("/login chatgpt", &mut context)
        .await
        .unwrap();
    assert!(matches!(
        result,
        Some(CommandResult::Login {
            provider: Some(provider)
        }) if provider == "chatgpt"
    ));
    let result_ag = SlashCommandHandler::handle("/login antigravity", &mut context)
        .await
        .unwrap();
    assert!(matches!(
        result_ag,
        Some(CommandResult::Login {
            provider: Some(provider)
        }) if provider == "antigravity"
    ));
}

#[tokio::test]
async fn model_switch_is_emitted_and_updates_configuration() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, mut events) = collecting_renderer();
    let mut context = SlashCommandContext {
        config: &mut config,
        auth_store: &mut auth,
        renderer: &renderer,
        session_id: None,
        session_manager: None,
    };
    let result = SlashCommandHandler::handle("/model gpt-4o openai", &mut context)
        .await
        .unwrap();

    assert!(matches!(result, Some(CommandResult::ModelChanged { .. })));
    assert_eq!(config.model, "gpt-4o");
    assert_eq!(config.provider, "openai");
    assert!(collected_output(&mut events).contains("Switched model to gpt-4o (openai)"));
}

#[tokio::test]
async fn session_command_prints_diagnostics() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, mut events) = collecting_renderer();
    let mut context = SlashCommandContext {
        config: &mut config,
        auth_store: &mut auth,
        renderer: &renderer,
        session_id: Some("session-diag-123"),
        session_manager: None,
    };

    let result = SlashCommandHandler::handle("/session", &mut context).await.unwrap();

    assert!(matches!(result, Some(CommandResult::Continue)));
    let output = collected_output(&mut events);
    assert!(output.contains("Session Diagnostics"));
    assert!(output.contains("Session ID:                  session-diag-123"));
}

#[tokio::test]
async fn compact_tree_and_rewind_commands_return_expected_results() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, _) = collecting_renderer();
    let mut context = SlashCommandContext {
        config: &mut config,
        auth_store: &mut auth,
        renderer: &renderer,
        session_id: None,
        session_manager: None,
    };

    let compact = SlashCommandHandler::handle("/compact keep tests only", &mut context)
        .await
        .unwrap();
    assert_eq!(
        compact,
        Some(CommandResult::Compact {
            instructions: Some("keep tests only".to_string())
        })
    );

    let tree = SlashCommandHandler::handle("/tree", &mut context).await.unwrap();
    assert_eq!(tree, Some(CommandResult::Tree));

    let rewind = SlashCommandHandler::handle("/rewind", &mut context).await.unwrap();
    assert_eq!(rewind, Some(CommandResult::Continue));
}
