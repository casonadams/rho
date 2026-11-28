use super::*;
use crate::config::Config;
use crate::repl::ReplSession;
use crate::repl::commands::{CommandResult, SlashCommandContext, SlashCommandHandler};
use crate::repl::line_mode::dispatch::compact_context;
use crate::repl::{collected_output, collecting_renderer};
use crate::ui::TerminalRenderer;
use rho_engine::auth::AuthStore;
use rho_engine::engine::eval::mock::{MockEngineConfig, mock_engine};
use rho_engine::provider::adapter::MockAdapter;
use rho_harness_core::provider::ModelRoute;
use rho_harness_core::session::SessionManager;
use std::sync::Arc;

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

async fn run_command(
    line: &str,
    config: &mut Config,
    session_id: &str,
    engine: Option<&crate::engine::AgentEngine>,
) -> (Option<CommandResult>, String) {
    let mut auth = AuthStore::default();
    let (renderer, mut events) = collecting_renderer();
    let mut context = SlashCommandContext {
        config,
        auth_store: &mut auth,
        renderer: &renderer,
        session_id: Some(session_id),
        session_manager: None,
        engine,
        home_dir: None,
    };
    let result = SlashCommandHandler::handle(line, &mut context).await.unwrap();
    (result, collected_output(&mut events))
}

#[tokio::test]
async fn compact_command_dispatches_with_optional_instructions() {
    let cases = [
        ("/compact", None),
        ("/compact focus on errors and tests", Some("focus on errors and tests")),
    ];
    for (line, instructions) in cases {
        let mut config = Config::default();
        let mut auth = AuthStore::default();
        let (renderer, _) = collecting_renderer();
        let mut context = test_context(&mut config, &mut auth, &renderer);

        let result = SlashCommandHandler::handle(line, &mut context).await.unwrap();
        assert_eq!(
            result,
            Some(CommandResult::Compact {
                instructions: instructions.map(str::to_string)
            })
        );
    }
}

async fn seed_long_turns(session_mgr: &SessionManager, sid: &str) {
    for i in 0..4 {
        let u = rho_harness_core::model::ChatMessage::user(format!(
            "Detailed query {i} with long description to consume context tokens"
        ));
        let a = rho_harness_core::model::ChatMessage::assistant(format!(
            "Comprehensive answer {i} analyzing the system and reviewing code"
        ));
        session_mgr.append_messages(sid, vec![u, a]).await.unwrap();
    }
}

async fn seed_session(config: &Config, messages: Vec<rho_harness_core::model::ChatMessage>) -> SessionManager {
    std::fs::create_dir_all(&config.sessions_dir).unwrap();
    let sm = SessionManager::new(&config.sessions_dir, None).unwrap();
    if !messages.is_empty() {
        sm.append_messages(&sm.session_id, messages).await.unwrap();
    }
    sm
}

async fn setup_compact_env(
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
    let sm = seed_session(&config, Vec::new()).await;
    let engine = mock_engine(
        Arc::new(MockAdapter::text(
            "## Goal\nAnalyze queries\n\n## Progress\nCompleted analyses",
        )),
        MockEngineConfig {
            session_manager: Some(sm),
            ..MockEngineConfig::new(temp, config.clone())
        },
    );
    let (renderer, events) = collecting_renderer();
    let mut session = ReplSession::new(config, AuthStore::default(), None);
    session.renderer = renderer;
    (session, engine, events)
}

#[tokio::test]
async fn compact_context_executes_and_prints_token_savings() {
    let temp = rho_test_support::TestDir::new();
    let (session, engine, mut events) = setup_compact_env(&temp).await;
    seed_long_turns(&engine.session_manager, &engine.session_manager.session_id).await;

    compact_context(&session, &engine, Some("preserve key decisions")).await;
    let output = collected_output(&mut events);
    assert!(output.contains("Compacting conversation context...") && output.contains("Compacted context:"));
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
async fn mcp_and_logout_commands_dispatch_by_renderer_mode() {
    let cases = [
        ("/mcp", true, CommandResult::OpenMcpSelector),
        ("/mcp login", true, CommandResult::Continue),
        (
            "/mcp login atlassian",
            true,
            CommandResult::Login {
                provider: Some("mcp:atlassian".to_string()),
            },
        ),
        (
            "/mcp logout atlassian",
            false,
            CommandResult::Logout {
                provider: Some("mcp:atlassian".to_string()),
            },
        ),
        ("/mcp", false, CommandResult::Continue),
        ("/logout", false, CommandResult::Logout { provider: None }),
        (
            "/logout chatgpt",
            true,
            CommandResult::Logout {
                provider: Some("chatgpt".to_string()),
            },
        ),
    ];
    for (line, interactive, expected) in cases {
        let mut config = Config::default();
        let mut auth = AuthStore::default();
        let renderer = if interactive {
            collecting_renderer().0
        } else {
            TerminalRenderer::default()
        };
        let mut context = test_context(&mut config, &mut auth, &renderer);
        let res = SlashCommandHandler::handle(line, &mut context).await.unwrap();
        assert_eq!(res, Some(expected), "{line}");
    }
}

#[tokio::test]
async fn model_switch_is_returned_for_the_switch_to_apply() {
    let cases = [
        ("/model gpt-4o openai", "", "gpt-4o", "openai"),
        (
            "/model claude:me@work.com/claude-opus-5",
            "claude",
            "claude-opus-5",
            "claude:me@work.com",
        ),
        ("/model gpt-4o", "openai:me@work.com", "gpt-4o", "openai:me@work.com"),
    ];
    for (input, current, model, provider) in cases {
        let mut config = Config {
            provider: current.to_string(),
            ..Config::default()
        };
        let mut auth = AuthStore::default();
        let (renderer, _events) = collecting_renderer();
        let mut context = test_context(&mut config, &mut auth, &renderer);

        let result = SlashCommandHandler::handle(input, &mut context).await.unwrap();

        assert_eq!(
            result,
            Some(CommandResult::ModelChanged {
                route: ModelRoute::new(provider, model),
            }),
            "{input}"
        );
    }
}

#[tokio::test]
async fn model_command_leaves_session_config_to_the_switch() {
    let mut config = Config {
        provider: "openai".to_string(),
        ..Config::default()
    };
    let before = (config.model.clone(), config.provider.clone());
    let mut auth = AuthStore::default();
    let (renderer, mut events) = collecting_renderer();
    let mut context = test_context(&mut config, &mut auth, &renderer);

    SlashCommandHandler::handle("/model gpt-4o", &mut context)
        .await
        .unwrap();

    assert_eq!((config.model.clone(), config.provider.clone()), before);
    assert_eq!(collected_output(&mut events), "");
}

#[tokio::test]
async fn model_command_keeps_the_selected_provider_that_lists_the_model() {
    let temp = std::env::temp_dir().join(format!("model_provider_{}", uuid::Uuid::new_v4()));
    let mut config = Config {
        provider: "ollama-cloud".to_string(),
        sessions_dir: temp.join("sessions"),
        ..Config::default()
    };
    let session_manager = seed_session(&config, Vec::new()).await;
    let session_id = session_manager.session_id.clone();
    let engine = mock_engine(
        Arc::new(MockAdapter::default()),
        MockEngineConfig {
            session_manager: Some(session_manager),
            ..MockEngineConfig::new(&temp, config.clone())
        },
    );
    engine.model_catalog().set(
        "ollama-cloud",
        rho_engine::provider::ProviderModels::Ready(vec![rho_engine::provider::DiscoveredModel::new(
            "deepseek-v4.1-flash",
            "deepseek-v4.1-flash",
            "1M ctx",
        )]),
    );

    let result = run_command("/model deepseek-v4.1-flash", &mut config, &session_id, Some(&engine))
        .await
        .0;
    assert_eq!(
        result,
        Some(CommandResult::ModelChanged {
            route: ModelRoute::new("ollama-cloud", "deepseek-v4.1-flash"),
        })
    );
    let _ = std::fs::remove_dir_all(temp);
}

#[tokio::test]
async fn tree_and_rewind_commands_return_expected_results() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, _) = collecting_renderer();
    let mut context = test_context(&mut config, &mut auth, &renderer);

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

    let cases = [
        (
            "/thinking OFF",
            Some(CommandResult::ThinkingChanged {
                level: Some("off".to_string()),
            }),
            Some("off"),
        ),
        ("/thinking hgih", Some(CommandResult::Continue), Some("off")),
    ];
    for (input, expected, level) in cases {
        assert_eq!(
            SlashCommandHandler::handle(input, &mut context).await.unwrap(),
            expected,
            "{input}"
        );
        assert_eq!(context.config.thinking_level.as_deref(), level, "{input}");
    }

    let think_modal_res = SlashCommandHandler::handle("/thinking", &mut context).await.unwrap();
    assert_eq!(think_modal_res, Some(CommandResult::OpenSettingsSelector));

    let fork_res = SlashCommandHandler::handle("/fork node_123", &mut context)
        .await
        .unwrap();
    assert_eq!(
        fork_res,
        Some(CommandResult::ForkSession {
            target: None,
            turn_or_node_id: Some("node_123".to_string())
        })
    );

    let fork_cases = [
        ("/fork", None, None),
        ("/fork 2", None, Some("2")),
        ("/fork pane", Some(ForkTarget::Pane), None),
        ("/fork PANE 2", Some(ForkTarget::Pane), Some("2")),
        ("/fork hsplit", Some(ForkTarget::HSplit), None),
        ("/fork vsplit abc", Some(ForkTarget::VSplit), Some("abc")),
        ("/fork window", Some(ForkTarget::Window), None),
        ("/fork tab 4", Some(ForkTarget::Tab), Some("4")),
    ];
    for (input, expected_target, expected_id) in fork_cases {
        let res = SlashCommandHandler::handle(input, &mut context).await.unwrap();
        assert_eq!(
            res,
            Some(CommandResult::ForkSession {
                target: expected_target,
                turn_or_node_id: expected_id.map(str::to_string),
            }),
            "{input}"
        );
    }

    let clone_res = SlashCommandHandler::handle("/clone", &mut context).await.unwrap();
    assert_eq!(clone_res, Some(CommandResult::CloneSession));
}

#[test]
fn is_slash_command_recognizes_root_commands_and_excludes_file_paths() {
    assert!(["/help", "/model gpt-4o"].into_iter().all(is_slash_command));
    assert!(
        ["/usr/bin/env", "/not_a_real_path/foo/bar", "hello world"]
            .into_iter()
            .all(|input| !is_slash_command(input))
    );
}

async fn setup_export_session(
    workspace: &std::path::Path,
    messages: Vec<rho_harness_core::model::ChatMessage>,
) -> (Config, SessionManager, String) {
    let config = Config {
        sessions_dir: workspace.join("sessions"),
        ..Config::default()
    };
    let sm = seed_session(&config, messages).await;
    let sid = sm.session_id.clone();
    (config, sm, sid)
}

#[tokio::test]
async fn export_command_writes_markdown_default_path() {
    let workspace = rho_test_support::TestDir::new();
    let msgs = vec![
        rho_harness_core::model::ChatMessage::user("hello for export"),
        rho_harness_core::model::ChatMessage::assistant("hello from the transcript"),
    ];
    let (mut config, session_manager, session_id) = setup_export_session(&workspace, msgs).await;

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
}

#[tokio::test]
async fn export_command_writes_html_to_override_path() {
    let workspace = rho_test_support::TestDir::new();
    let override_path = workspace.join("out/transcript.html");
    let (mut config, session_manager, session_id) = setup_export_session(&workspace, Vec::new()).await;

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
}

#[tokio::test]
async fn export_command_rejects_unknown_format_with_usage() {
    let workspace = rho_test_support::TestDir::new();
    let mut config = Config {
        sessions_dir: workspace.join("sessions"),
        ..Config::default()
    };
    let session_manager = seed_session(&config, Vec::new()).await;
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
}

#[tokio::test]
async fn session_command_prints_diagnostics() {
    let mut config = Config::default();
    let (result, output) = run_command("/session", &mut config, "session-diag-123", None).await;

    assert!(matches!(result, Some(CommandResult::Continue)));
    assert!(output.contains("Session Diagnostics"));
    assert!(output.contains("Session ID:                  session-diag-123"));
}

async fn setup_engine_session(temp: &std::path::Path, config: Config) -> (crate::engine::AgentEngine, String) {
    let sm = seed_session(&config, Vec::new()).await;
    let sid = sm.session_id.clone();
    let engine = mock_engine(
        Arc::new(MockAdapter::default()),
        MockEngineConfig {
            session_manager: Some(sm),
            ..MockEngineConfig::new(temp, config)
        },
    );
    (engine, sid)
}

#[tokio::test]
async fn session_command_prints_diagnostics_with_engine() {
    let temp = rho_test_support::TestDir::new();
    let mut config = Config {
        sessions_dir: temp.join("sessions"),
        thinking_level: Some("high".to_string()),
        ..Config::default()
    };
    let (engine, session_id) = setup_engine_session(&temp, config.clone()).await;

    let (result, output) = run_command("/session", &mut config, &session_id, Some(&engine)).await;
    assert!(matches!(result, Some(CommandResult::Continue)));
    assert!(output.contains("Session Diagnostics") && output.contains("Thinking Level:"));
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
    let workspace = rho_test_support::TestDir::new();
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
    let workspace = rho_test_support::TestDir::new();
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
}

fn usage_engine(
    base_dir: &std::path::Path,
    config: &Config,
    step: rho_engine::engine::metrics::StructuralUsage,
) -> crate::engine::AgentEngine {
    let engine = mock_engine(
        Arc::new(MockAdapter::text("test response")),
        MockEngineConfig::new(base_dir, config.clone()),
    );
    engine
        .usage()
        .record_turn(rho_engine::engine::tracking::TurnUsage::single(step), 500);
    engine
}

async fn run_usage_command(command: &str, step: rho_engine::engine::metrics::StructuralUsage) -> String {
    let dir = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    let engine = usage_engine(dir.path(), &config, step);
    let (result, output) = run_command(command, &mut config, "test-session", Some(&engine)).await;
    assert_eq!(result, Some(CommandResult::Continue));
    output
}

#[tokio::test]
async fn usage_commands_display_cache_stats_when_present() {
    let cases: [(&str, rho_engine::engine::metrics::StructuralUsage, &[&str]); 2] = [
        (
            "/tokens",
            rho_engine::engine::metrics::StructuralUsage {
                input_tokens: 100,
                output_tokens: 50,
                cached_input_tokens: Some(900),
                cache_creation_input_tokens: Some(100),
                ..Default::default()
            },
            &["Tokens:", "cache: R900 W100", "Prompt Cache Hit:            90.0%"],
        ),
        (
            "/session",
            rho_engine::engine::metrics::StructuralUsage {
                input_tokens: 1_000,
                output_tokens: 200,
                cached_input_tokens: Some(9_000),
                ..Default::default()
            },
            &[
                "Cache Efficiency:            cache hit: 90%",
                "Prompt Cache Hit:            90.0%",
            ],
        ),
    ];
    for (command, step, expected) in cases {
        let output = run_usage_command(command, step).await;
        for &needle in expected {
            assert!(output.contains(needle), "{command}: {output}");
        }
    }
}

#[test]
fn test_parse_command_args() {
    use super::args::parse_command_args;

    assert_eq!(parse_command_args(""), Vec::<String>::new());
    assert_eq!(parse_command_args("   "), Vec::<String>::new());
    assert_eq!(parse_command_args("hello"), vec!["hello"]);
    assert_eq!(parse_command_args("foo bar  baz"), vec!["foo", "bar", "baz"]);
    assert_eq!(
        parse_command_args("cmd \"hello world\" 'single quote' tail"),
        vec!["cmd", "hello world", "single quote", "tail"]
    );
    assert_eq!(
        parse_command_args("echo \"unclosed quote"),
        vec!["echo", "unclosed quote"]
    );
}

#[test]
fn test_prompt_model_choice_from() {
    use super::model::prompt_model_choice_from;
    use std::io::Cursor;

    let mut reader = Cursor::new(b"1\n");
    let mut writer = Vec::new();
    assert_eq!(prompt_model_choice_from(&mut reader, &mut writer, &[]), None);

    let models = vec![
        "gpt-4o (openai) - Flagship model".to_string(),
        "claude-3-5-sonnet (anthropic) - Smart model".to_string(),
    ];

    let mut reader2 = Cursor::new(b"2\n");
    let mut writer2 = Vec::new();
    let choice = prompt_model_choice_from(&mut reader2, &mut writer2, &models);
    assert_eq!(choice, Some(("claude-3-5-sonnet".to_string(), "anthropic".to_string())));

    let mut reader3 = Cursor::new(b"99\n");
    let mut writer3 = Vec::new();
    assert_eq!(prompt_model_choice_from(&mut reader3, &mut writer3, &models), None);
}

#[test]
fn test_prompt_skill_choice_from() {
    use super::skills::prompt_skill_choice_from;
    use std::io::Cursor;

    let mut reader = Cursor::new(b"1\n");
    let mut writer = Vec::new();
    assert_eq!(prompt_skill_choice_from(&mut reader, &mut writer, &[]), None);

    let skills = vec![
        "review - Code reviewer (built-in)".to_string(),
        "lint - Linter (custom)".to_string(),
    ];

    let mut reader2 = Cursor::new(b"1\n");
    let mut writer2 = Vec::new();
    let choice = prompt_skill_choice_from(&mut reader2, &mut writer2, &skills);
    assert_eq!(choice, Some("review".to_string()));

    let mut reader3 = Cursor::new(b"abc\n");
    let mut writer3 = Vec::new();
    assert_eq!(prompt_skill_choice_from(&mut reader3, &mut writer3, &skills), None);
}

#[test]
fn registry_has_unique_names_and_aliases() {
    use std::collections::HashSet;

    let mut seen_ids = HashSet::new();
    let mut seen_names = HashSet::new();
    for spec in BUILTIN_COMMANDS {
        assert!(seen_ids.insert(spec.id), "duplicate BuiltinCommand {:?}", spec.id);
        assert!(seen_names.insert(spec.name), "duplicate name {}", spec.name);
        assert!(!spec.usage.is_empty());
        assert!(!spec.summary.is_empty());
        assert!(!spec.detail.is_empty());
        assert_eq!(lookup_builtin(spec.name).map(|s| s.id), Some(spec.id));
        assert_eq!(lookup_builtin(&format!("/{}", spec.name)).map(|s| s.id), Some(spec.id));
        for alias in spec.aliases {
            assert!(seen_names.insert(alias), "duplicate alias {alias}");
            assert_eq!(lookup_builtin(alias).map(|s| s.id), Some(spec.id));
            assert_eq!(lookup_builtin(&alias.to_uppercase()).map(|s| s.id), Some(spec.id));
        }
    }
}

#[tokio::test]
async fn help_with_command_argument_prints_extended_guide() {
    let mut config = Config::default();

    let (res, out) = run_command("/help mcp", &mut config, "s1", None).await;
    assert_eq!(res, Some(CommandResult::Continue));
    assert!(out.contains("/mcp"));
    assert!(out.contains("Subcommands"));
    assert!(out.contains("/mcp login <server>"));

    let (res, out) = run_command("/help /worktree", &mut config, "s1", None).await;
    assert_eq!(res, Some(CommandResult::Continue));
    assert!(out.contains("/wt"));
    assert!(out.contains("Aliases:"));
    assert!(out.contains("/worktree"));

    let (res, out) = run_command("/help settings", &mut config, "s1", None).await;
    assert_eq!(res, Some(CommandResult::Continue));
    assert!(out.contains("Available while a turn is running"));

    let (res, out) = run_command("/help nope", &mut config, "s1", None).await;
    assert_eq!(res, Some(CommandResult::Continue));
    assert!(out.contains("Unknown command: /nope"));
}

#[tokio::test]
async fn models_tools_and_agents_dispatch_in_tui_mode() {
    let mut config = Config::default();
    let cases = [
        ("/models", CommandResult::OpenModelsSelector),
        ("/tools", CommandResult::OpenToolsSelector),
        ("/permissions", CommandResult::OpenToolsSelector),
        ("/agents", CommandResult::OpenAgentsSelector),
    ];
    for (cmd, expected) in cases {
        let (res, _) = run_command(cmd, &mut config, "s1", None).await;
        assert_eq!(res, Some(expected), "failed for {cmd}");
    }
}

#[test]
fn mid_turn_policy_classifies_immediate_vs_queued() {
    for cmd in [
        "/settings",
        "/help",
        "/help mcp",
        "/model",
        "/model gpt-4o",
        "/models",
        "/thinking high",
        "/think off",
        "/tools",
        "/permissions",
        "/agents",
        "/session",
        "/tokens",
        "/fork pane",
        "/fork hsplit 2",
        "/fork vsplit",
        "/fork window",
        "/fork TAB node_1",
        "/btw",
        "/btw foo",
    ] {
        assert_eq!(
            mid_turn_policy(cmd),
            MidTurnPolicy::Immediate,
            "expected immediate: {cmd}"
        );
    }
    for cmd in [
        "/compact",
        "/clear",
        "/new",
        "/rewind 1",
        "/fork",
        "/fork 2",
        "/fork abc",
        "/wt",
        "/unknown",
    ] {
        assert_eq!(mid_turn_policy(cmd), MidTurnPolicy::Queue, "expected queued: {cmd}");
    }
}

#[test]
fn help_arg_completion_completes_builtin_commands() {
    let completions = crate::repl::interactive::CompletionSet::from_sources(Default::default());
    let matches = completions.complete("/help set", "/help set".len());
    assert!(matches.iter().any(|c| c.value == "/help settings"));
}

#[tokio::test]
async fn btw_command_parses_optional_question() {
    let cases = [
        ("/btw", None),
        ("/btw   ", None),
        ("/btw what changed?", Some("what changed?")),
        ("/btw  keep   spacing\nline2 ", Some("keep   spacing\nline2")),
    ];
    for (line, expected) in cases {
        let mut config = Config::default();
        let mut auth = AuthStore::default();
        let (renderer, _) = collecting_renderer();
        let mut context = test_context(&mut config, &mut auth, &renderer);

        let result = SlashCommandHandler::handle(line, &mut context).await.unwrap();
        assert_eq!(
            result,
            Some(CommandResult::Btw {
                question: expected.map(str::to_string)
            })
        );
    }
}

#[tokio::test]
async fn recap_command_parses_and_is_immediate_mid_turn() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, _) = collecting_renderer();
    let mut context = test_context(&mut config, &mut auth, &renderer);

    let result = SlashCommandHandler::handle("/recap", &mut context).await.unwrap();
    assert_eq!(result, Some(CommandResult::Recap));
    assert_eq!(mid_turn_policy("/recap"), MidTurnPolicy::Immediate);
}
