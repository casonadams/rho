use super::submitted_input_rows;

#[test]
fn slash_commands_complete_from_a_prefix() {
    let sources = crate::repl::interactive::CompletionSources::new().with_templates(vec!["review".to_string()]);
    let completions = crate::repl::interactive::CompletionSet::from_sources(sources);
    let suggestions = completions.complete("/mod", 4);
    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].value, "/model");

    let tmpl_suggestions = completions.complete("/rev", 4);
    assert_eq!(tmpl_suggestions.len(), 1);
    assert_eq!(tmpl_suggestions[0].value, "/review");
}

#[test]
fn skill_names_complete_from_prefix() {
    let skill = rho_harness_core::skills::ResolvedSkill {
        metadata: rho_harness_core::skills::SkillMetadata {
            name: "plan".to_string(),
            description: "Planning workflow".to_string(),
            location: "/path".to_string(),
            disable_model_invocation: false,
        },
        origin: rho_harness_core::skills::SkillOrigin::User,
    };
    let sources = crate::repl::interactive::CompletionSources::new().with_skills(vec![skill]);
    let completions = crate::repl::interactive::CompletionSet::from_sources(sources);
    let suggestions = completions.complete("/skill pl", 9);
    assert!(suggestions.iter().any(|s| s.value == "/skill plan"));
}

#[test]
fn submitted_input_rows_include_prompt_width_and_terminal_wrapping() {
    assert_eq!(submitted_input_rows("hello", 80), 1);
    assert_eq!(submitted_input_rows(&"x".repeat(78), 80), 2);
    assert_eq!(submitted_input_rows("one\ntwo", 80), 2);
    assert_eq!(submitted_input_rows("界界", 5), 2);
}

async fn setup_reload_session(home: &std::path::Path) -> (crate::repl::ReplSession, crate::engine::AgentEngine) {
    use crate::auth::AuthStore;
    use crate::config::cli::Cli;
    use clap::Parser;
    use rho_harness_core::config::Config;

    let config = Config::load(None).unwrap();
    config.ensure_dirs().unwrap();
    let mut session = crate::repl::ReplSession::new(config, AuthStore::default(), None)
        .with_cli(Some(Cli::parse_from(["rho", "--max-turns", "9"])));
    session.config.model = "runtime-model".to_string();
    let engine = crate::platform::agent_engine(session.config.clone(), session.auth_store.clone(), None)
        .await
        .unwrap();
    std::fs::write(home.join("config.toml"), "max_turns = 42\n").unwrap();
    (session, engine)
}

#[tokio::test]
async fn reload_adopts_file_and_cli_values_but_keeps_runtime_model() {
    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _env = ENV_LOCK.lock().await;
    unsafe {
        std::env::set_var("ANTHROPIC_API_KEY", "test-key-not-real");
    }
    let home = std::env::temp_dir().join(format!("repl_reload_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&home).unwrap();
    unsafe {
        std::env::set_var("RHO_HOME", &home);
    }

    let (mut session, engine) = setup_reload_session(&home).await;
    let reloaded = session.reload_engine(&engine).await.unwrap();

    assert_eq!(
        (reloaded.config.model.as_str(), reloaded.config.max_turns),
        ("runtime-model", 9)
    );
    assert_eq!(
        (session.config.max_turns, session.config.model.as_str()),
        (9, "runtime-model")
    );

    unsafe {
        std::env::remove_var("RHO_HOME");
    }
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn format_footer_stats_includes_cached_read_and_write_tokens() {
    let footer = crate::repl::runner::FooterInfo {
        model: "claude-3-7-sonnet".to_string(),
        provider: "anthropic".to_string(),
        thinking: Some("medium".to_string()),
        total_input: 1_200,
        total_output: 450,
        total_cache_read: 10_000,
        total_cache_write: 2_000,
        context_window: 200_000,
        context_percent: Some(6.1),
        tokens_per_second: Some(45.0),
        quota: None,
        path_left: String::new(),
    };
    let line = crate::repl::runner::format_footer_stats(&footer, 100);
    assert!(line.contains("↑1.2k"), "should contain input tokens: {line}");
    assert!(line.contains("↓450"), "should contain output tokens: {line}");
    assert!(line.contains("R10.0k"), "should contain cache read: {line}");
    assert!(line.contains("W2.0k"), "should contain cache write: {line}");
    assert!(line.contains("6.1%/200k"), "should contain context info: {line}");
    assert!(line.contains("@45t/s"), "should contain speed: {line}");
    assert!(
        line.contains("claude-3-7-sonnet · medium"),
        "should contain model and thinking: {line}"
    );
}

#[test]
fn format_footer_stats_omits_cache_markers_when_zero() {
    let footer = crate::repl::runner::FooterInfo {
        model: "gpt-4o".to_string(),
        provider: "openai".to_string(),
        thinking: None,
        total_input: 500,
        total_output: 100,
        total_cache_read: 0,
        total_cache_write: 0,
        context_window: 128_000,
        context_percent: Some(0.4),
        tokens_per_second: None,
        quota: None,
        path_left: String::new(),
    };
    let line = crate::repl::runner::format_footer_stats(&footer, 80);
    assert!(line.contains("↑500"));
    assert!(line.contains("↓100"));
    assert!(!line.contains("R0"), "should not contain R0: {line}");
    assert!(!line.contains("W0"), "should not contain W0: {line}");
    assert!(line.contains("gpt-4o"));
}
