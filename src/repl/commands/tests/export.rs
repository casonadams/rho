use super::{collected_output, collecting_renderer};
use crate::config::Config;
use crate::repl::commands::{CommandResult, SlashCommandContext, SlashCommandHandler};
use rho_engine::auth::AuthStore;

async fn setup_export_session(
    workspace: &std::path::Path,
) -> (Config, rho_harness_core::session::SessionManager, String) {
    let config = Config {
        sessions_dir: workspace.join("sessions"),
        ..Config::default()
    };
    std::fs::create_dir_all(&config.sessions_dir).unwrap();
    let sm = rho_harness_core::session::SessionManager::new(&config.sessions_dir, None).unwrap();
    let sid = sm.session_id.clone();
    (config, sm, sid)
}

#[tokio::test]
async fn export_command_writes_markdown_default_path() {
    let workspace = std::env::temp_dir().join(format!("export_cmd_{}", uuid::Uuid::new_v4()));
    let (mut config, session_manager, session_id) = setup_export_session(&workspace).await;
    use rig::memory::ConversationMemory;
    let msgs = vec![
        rig::message::Message::user("hello for export"),
        rig::message::Message::assistant("hello from the transcript"),
    ];
    session_manager.append(&session_id, msgs).await.unwrap();

    let (mut auth, (renderer, mut events)) = (AuthStore::default(), collecting_renderer());
    let mut context = SlashCommandContext {
        config: &mut config,
        auth_store: &mut auth,
        renderer: &renderer,
        session_id: Some(&session_id),
        session_manager: Some(&session_manager),
        engine: None,
        home_dir: None,
    };

    let result = SlashCommandHandler::handle("/export", &mut context).await.unwrap();
    assert!(matches!(result, Some(CommandResult::Continue)));

    let written = config.sessions_dir.join(format!("{session_id}.md"));
    let content = std::fs::read_to_string(&written).unwrap();
    assert!(content.contains("# rho session:") && content.contains("hello for export"));
    assert!(collected_output(&mut events).contains("[Exported session to"));
    std::fs::remove_dir_all(workspace).unwrap();
}

#[tokio::test]
async fn export_command_writes_html_to_override_path() {
    let workspace = std::env::temp_dir().join(format!("export_cmd_{}", uuid::Uuid::new_v4()));
    let override_path = workspace.join("out/transcript.html");
    let (mut config, session_manager, session_id) = setup_export_session(&workspace).await;

    let (mut auth, (renderer, _)) = (AuthStore::default(), collecting_renderer());
    let mut context = SlashCommandContext {
        config: &mut config,
        auth_store: &mut auth,
        renderer: &renderer,
        session_id: Some(&session_id),
        session_manager: Some(&session_manager),
        engine: None,
        home_dir: None,
    };

    let cmd = format!("/export html {}", override_path.display());
    let result = SlashCommandHandler::handle(&cmd, &mut context).await.unwrap();
    assert!(matches!(result, Some(CommandResult::Continue)));
    assert!(
        std::fs::read_to_string(&override_path)
            .unwrap()
            .starts_with("<!doctype html>")
    );
    std::fs::remove_dir_all(workspace).unwrap();
}

#[tokio::test]
async fn export_command_rejects_unknown_format_with_usage() {
    let workspace = std::env::temp_dir().join(format!("export_cmd_{}", uuid::Uuid::new_v4()));
    let mut config = Config {
        sessions_dir: workspace.join("sessions"),
        ..Config::default()
    };
    std::fs::create_dir_all(&config.sessions_dir).unwrap();
    let session_manager = rho_harness_core::session::SessionManager::new(&config.sessions_dir, None).unwrap();
    let session_id = session_manager.session_id.clone();

    let mut auth = AuthStore::default();
    let (renderer, mut events) = collecting_renderer();
    let mut context = SlashCommandContext {
        config: &mut config,
        auth_store: &mut auth,
        renderer: &renderer,
        session_id: Some(&session_id),
        session_manager: Some(&session_manager),
        engine: None,
        home_dir: None,
    };

    let result = SlashCommandHandler::handle("/export xml", &mut context).await.unwrap();
    assert!(matches!(result, Some(CommandResult::Continue)));
    assert!(collected_output(&mut events).contains("Usage: /export"));

    std::fs::remove_dir_all(workspace).unwrap();
}
