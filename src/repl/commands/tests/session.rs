use super::{collected_output, collecting_renderer};
use crate::config::Config;
use crate::repl::commands::{CommandResult, SlashCommandContext, SlashCommandHandler};
use rho_engine::auth::AuthStore;

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
        engine: None,
        home_dir: None,
    };

    let result = SlashCommandHandler::handle("/session", &mut context).await.unwrap();

    assert!(matches!(result, Some(CommandResult::Continue)));
    let output = collected_output(&mut events);
    assert!(output.contains("Session Diagnostics"));
    assert!(output.contains("Session ID:                  session-diag-123"));
}

async fn setup_engine_session(temp: &std::path::Path, config: Config) -> (crate::engine::AgentEngine, String) {
    std::fs::create_dir_all(&config.sessions_dir).unwrap();
    let sm = rho_harness_core::session::SessionManager::new(&config.sessions_dir, None).unwrap();
    let sid = sm.session_id.clone();
    let engine = rho_engine::engine::eval::mock::mock_engine_with_session(
        rig::test_utils::MockCompletionModel::default(),
        rho_engine::engine::eval::mock::MockEngineConfig {
            base_dir: temp,
            app_config: config,
            session_manager: Some(sm),
            built_in_tools: None,
        },
    );
    (engine, sid)
}

#[tokio::test]
async fn session_command_prints_diagnostics_with_engine() {
    let temp = std::env::temp_dir().join(format!("session_diag_{}", uuid::Uuid::new_v4()));
    let mut config = Config {
        sessions_dir: temp.join("sessions"),
        thinking_level: Some("high".to_string()),
        ..Config::default()
    };
    let mut auth = AuthStore::default();
    let (renderer, mut events) = collecting_renderer();
    let (engine, session_id) = setup_engine_session(&temp, config.clone()).await;

    let mut context = SlashCommandContext {
        config: &mut config,
        auth_store: &mut auth,
        renderer: &renderer,
        session_id: Some(&session_id),
        session_manager: None,
        engine: Some(&engine),
        home_dir: None,
    };

    let result = SlashCommandHandler::handle("/session", &mut context).await.unwrap();
    assert!(matches!(result, Some(CommandResult::Continue)));
    let output = collected_output(&mut events);
    assert!(output.contains("Session Diagnostics") && output.contains("Thinking Level:"));
    let _ = std::fs::remove_dir_all(temp);
}
