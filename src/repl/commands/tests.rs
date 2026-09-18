use super::*;
use crate::config::Config;
use crate::repl::ReplSession;
use crate::repl::commands::{CommandResult, SlashCommandContext, SlashCommandHandler};
use crate::repl::line_mode::dispatch::compact_context;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{InteractiveUi, OutputEvent, UiEvent};
use rho_engine::auth::AuthStore;
use rho_engine::engine::eval::mock::{MockEngineConfig, mock_engine_with_session};
use rho_harness_core::session::SessionManager;
use rig::memory::ConversationMemory;
use rig::message::Message;
use rig::test_utils::MockCompletionModel;
use tokio::sync::mpsc;

fn collecting_renderer() -> (TerminalRenderer, mpsc::UnboundedReceiver<UiEvent>) {
    let (ui, events) = InteractiveUi::channel();
    (TerminalRenderer::with_ui(ui), events)
}

fn collected_output(events: &mut mpsc::UnboundedReceiver<UiEvent>) -> String {
    std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event {
            UiEvent::Output(OutputEvent::Text(text) | OutputEvent::StreamText(text)) => Some(text),
            UiEvent::Transcript(crate::ui::interactive::TranscriptItem::Notice(text)) => Some(text),
            UiEvent::SystemMessage(Some(text)) => Some(text),
            _ => None,
        })
        .collect()
}

fn test_context<'a>(
    config: &'a mut Config,
    auth_store: &'a mut AuthStore,
    renderer: &'a TerminalRenderer,
) -> SlashCommandContext<'a> {
    SlashCommandContext {
        config,
        auth_store,
        renderer,
        session_id: None,
        session_manager: None,
        engine: None,
        home_dir: None,
    }
}

#[tokio::test]
async fn compact_command_without_instructions_dispatches() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, _) = collecting_renderer();
    let mut context = test_context(&mut config, &mut auth, &renderer);

    let result = SlashCommandHandler::handle("/compact", &mut context).await.unwrap();
    assert_eq!(result, Some(CommandResult::Compact { instructions: None }));
}

#[tokio::test]
async fn compact_command_with_instructions_dispatches() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, _) = collecting_renderer();
    let mut context = test_context(&mut config, &mut auth, &renderer);

    let result = SlashCommandHandler::handle("/compact focus on errors and tests", &mut context)
        .await
        .unwrap();

    assert_eq!(
        result,
        Some(CommandResult::Compact {
            instructions: Some("focus on errors and tests".to_string())
        })
    );
}

async fn seed_long_turns(session_mgr: &SessionManager, sid: &str) {
    for i in 0..4 {
        let u = Message::user(format!(
            "Detailed query {i} with long description to consume context tokens"
        ));
        let a = Message::assistant(format!(
            "Comprehensive answer {i} analyzing the system and reviewing code"
        ));
        session_mgr.append(sid, vec![u, a]).await.unwrap();
    }
}

fn setup_compact_env(
    temp: &std::path::Path,
) -> (
    ReplSession,
    crate::engine::AgentEngine,
    tokio::sync::mpsc::UnboundedReceiver<crate::ui::interactive::UiEvent>,
) {
    let config = Config {
        sessions_dir: temp.join("sessions"),
        keep_recent_tokens: 10,
        ..Config::default()
    };
    std::fs::create_dir_all(&config.sessions_dir).unwrap();
    let sm = SessionManager::new(&config.sessions_dir, None).unwrap();
    let engine = mock_engine_with_session(
        MockCompletionModel::text("## Goal\nAnalyze queries\n\n## Progress\nCompleted analyses"),
        MockEngineConfig {
            base_dir: temp,
            app_config: config.clone(),
            session_manager: Some(sm),
            built_in_tools: None,
        },
    );
    let (renderer, events) = collecting_renderer();
    let mut session = ReplSession::new(config, AuthStore::default(), None);
    session.renderer = renderer;
    (session, engine, events)
}

#[tokio::test]
async fn compact_context_executes_and_prints_token_savings() {
    let temp = std::env::temp_dir().join(format!("compact_cmd_{}", uuid::Uuid::new_v4()));
    let (session, engine, mut events) = setup_compact_env(&temp);
    seed_long_turns(&engine.session_manager, &engine.session_manager.session_id).await;

    compact_context(&session, &engine, Some("preserve key decisions")).await;
    let output = collected_output(&mut events);
    assert!(output.contains("[Compacting conversation context...]") && output.contains("[Compacted context:"));
    let _ = std::fs::remove_dir_all(temp);
}

#[tokio::test]
async fn help_is_emitted_through_the_renderer() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, _) = collecting_renderer();
    let mut context = test_context(&mut config, &mut auth, &renderer);

    let result = SlashCommandHandler::handle("/help", &mut context).await.unwrap();
    assert_eq!(result, Some(CommandResult::OpenHelpSelector));

    let non_interactive_renderer = TerminalRenderer::default();
    let mut non_interactive_context = test_context(&mut config, &mut auth, &non_interactive_renderer);
    let non_interactive_res = SlashCommandHandler::handle("/help", &mut non_interactive_context)
        .await
        .unwrap();
    assert_eq!(non_interactive_res, Some(CommandResult::Continue));
}

#[tokio::test]
async fn login_is_dispatched_without_collecting_credentials() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, _) = collecting_renderer();
    let mut context = test_context(&mut config, &mut auth, &renderer);

    for provider in ["chatgpt", "antigravity", "claude"] {
        let cmd = format!("/login {provider}");
        let res = SlashCommandHandler::handle(&cmd, &mut context).await.unwrap();
        assert!(matches!(res, Some(CommandResult::Login { provider: Some(p) }) if p == provider));
    }

    let bare_res = SlashCommandHandler::handle("/login", &mut context).await.unwrap();
    assert_eq!(bare_res, Some(CommandResult::OpenLoginSelector));
}

#[tokio::test]
async fn model_switch_is_emitted_and_updates_configuration() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, mut events) = collecting_renderer();
    let mut context = test_context(&mut config, &mut auth, &renderer);

    let result = SlashCommandHandler::handle("/model gpt-4o openai", &mut context)
        .await
        .unwrap();

    assert!(matches!(result, Some(CommandResult::ModelChanged { .. })));
    assert_eq!(config.model, "gpt-4o");
    assert_eq!(config.provider, "openai");
    assert!(collected_output(&mut events).contains("Model: gpt-4o (openai)"));
}

#[tokio::test]
async fn compact_tree_and_rewind_commands_return_expected_results() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, _) = collecting_renderer();
    let mut context = test_context(&mut config, &mut auth, &renderer);

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
    assert_eq!(tree, Some(CommandResult::OpenTreeSelector));

    let rewind = SlashCommandHandler::handle("/rewind", &mut context).await.unwrap();
    assert_eq!(rewind, Some(CommandResult::Continue));
}

#[tokio::test]
async fn reload_command_requests_engine_reload() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, _) = collecting_renderer();
    let mut context = test_context(&mut config, &mut auth, &renderer);

    let result = SlashCommandHandler::handle("/reload", &mut context).await.unwrap();
    assert_eq!(result, Some(CommandResult::Reload));

    let with_args = SlashCommandHandler::handle("/reload now", &mut context).await.unwrap();
    assert_eq!(with_args, Some(CommandResult::Reload));
}

#[tokio::test]
async fn test_new_and_thinking_commands() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, _) = collecting_renderer();
    let mut context = test_context(&mut config, &mut auth, &renderer);

    let new_res = SlashCommandHandler::handle("/new", &mut context).await.unwrap();
    assert_eq!(new_res, Some(CommandResult::ClearContext));

    let remote_res = SlashCommandHandler::handle("/remote", &mut context).await.unwrap();
    assert_eq!(remote_res, Some(CommandResult::OpenRemoteModal));

    let think_res = SlashCommandHandler::handle("/thinking high", &mut context)
        .await
        .unwrap();
    assert_eq!(
        think_res,
        Some(CommandResult::ThinkingChanged {
            level: Some("high".to_string())
        })
    );
    assert_eq!(context.config.thinking_level.as_deref(), Some("high"));

    let think_modal_res = SlashCommandHandler::handle("/thinking", &mut context).await.unwrap();
    assert_eq!(think_modal_res, Some(CommandResult::OpenSettingsSelector));

    let fork_res = SlashCommandHandler::handle("/fork node_123", &mut context)
        .await
        .unwrap();
    assert_eq!(
        fork_res,
        Some(CommandResult::ForkSession {
            turn_or_node_id: Some("node_123".to_string())
        })
    );

    let clone_res = SlashCommandHandler::handle("/clone", &mut context).await.unwrap();
    assert_eq!(clone_res, Some(CommandResult::CloneSession));
}

#[test]
fn is_slash_command_recognizes_root_commands_and_excludes_file_paths() {
    assert!(is_slash_command("/help"));
    assert!(is_slash_command("/model gpt-4o"));
    assert!(!is_slash_command("/usr/bin/env"));
    assert!(!is_slash_command("/not_a_real_path/foo/bar"));
    assert!(!is_slash_command("hello world"));
}

async fn setup_export_session(workspace: &std::path::Path) -> (Config, SessionManager, String) {
    let config = Config {
        sessions_dir: workspace.join("sessions"),
        ..Config::default()
    };
    std::fs::create_dir_all(&config.sessions_dir).unwrap();
    let sm = SessionManager::new(&config.sessions_dir, None).unwrap();
    let sid = sm.session_id.clone();
    (config, sm, sid)
}

#[tokio::test]
async fn export_command_writes_markdown_default_path() {
    let workspace = std::env::temp_dir().join(format!("export_cmd_{}", uuid::Uuid::new_v4()));
    let (mut config, session_manager, session_id) = setup_export_session(&workspace).await;
    let msgs = vec![
        Message::user("hello for export"),
        Message::assistant("hello from the transcript"),
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
    let session_manager = SessionManager::new(&config.sessions_dir, None).unwrap();
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

    let result = SlashCommandHandler::handle("/export unknown_format", &mut context)
        .await
        .unwrap();
    assert!(matches!(result, Some(CommandResult::Continue)));
    assert!(collected_output(&mut events).contains("Usage: /export [html|md] [path]"));
    std::fs::remove_dir_all(workspace).unwrap();
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
        engine: None,
        home_dir: None,
    };

    let result = SlashCommandHandler::handle("/session", &mut context).await.unwrap();

    assert!(matches!(result, Some(CommandResult::Continue)));
    let output = collected_output(&mut events);
    assert!(output.contains("Session Diagnostics"));
    assert!(output.contains("Session ID:                  session-diag-123"));
    assert!(output.contains("Semantic Search (RAG):       Disabled"));
}

async fn setup_engine_session(temp: &std::path::Path, config: Config) -> (crate::engine::AgentEngine, String) {
    std::fs::create_dir_all(&config.sessions_dir).unwrap();
    let sm = SessionManager::new(&config.sessions_dir, None).unwrap();
    let sid = sm.session_id.clone();
    let engine = mock_engine_with_session(
        MockCompletionModel::default(),
        MockEngineConfig {
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

fn write_test_skill(workspace: &std::path::Path, name: &str, body: &str) {
    let dir = workspace.join(".agents").join("skills").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("SKILL.md"), body).unwrap();
}

fn skill_context<'a>(
    config: &'a mut Config,
    auth: &'a mut AuthStore,
    renderer: &'a TerminalRenderer,
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
    let mut context = skill_context(&mut config, &mut auth, &renderer, Some(&workspace));

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
    let mut context = skill_context(&mut config, &mut auth, &renderer, None);

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
    let mut context = skill_context(&mut config, &mut auth, &renderer, Some(&workspace));

    let result = SlashCommandHandler::handle("/skill:my-flow create foo", &mut context)
        .await
        .unwrap();
    let Some(CommandResult::ExpandedPrompt { text }) = result else {
        panic!("expected ExpandedPrompt");
    };
    assert!(text.contains("Run step A then step B") && text.contains("Skill input: create foo"));
    let _ = std::fs::remove_dir_all(workspace);
}
