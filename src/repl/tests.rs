use super::completer::RhoCompleter;
use super::submitted_input_rows;
use reedline::Completer;

#[test]
fn slash_commands_complete_from_a_prefix() {
    let sources = crate::repl::interactive::CompletionSources::new().with_templates(vec!["review".to_string()]);
    let mut completer = RhoCompleter::new(sources);
    let suggestions = completer.complete("/mo", 3);
    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].value, "/model");

    let tmpl_suggestions = completer.complete("/rev", 4);
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
        },
        origin: rho_harness_core::skills::SkillOrigin::User,
    };
    let sources = crate::repl::interactive::CompletionSources::new().with_skills(vec![skill]);
    let mut completer = RhoCompleter::new(sources);
    let suggestions = completer.complete("/skill pl", 9);
    assert!(suggestions.iter().any(|s| s.value == "/skill plan"));
}

#[test]
fn submitted_input_rows_include_prompt_width_and_terminal_wrapping() {
    assert_eq!(submitted_input_rows("hello", 80), 1);
    assert_eq!(submitted_input_rows(&"x".repeat(78), 80), 2);
    assert_eq!(submitted_input_rows("one\ntwo", 80), 2);
    assert_eq!(submitted_input_rows("界界", 5), 2);
}

#[tokio::test]
async fn reload_adopts_file_and_cli_values_but_keeps_runtime_model() {
    use crate::auth::AuthStore;
    use crate::config::cli::Cli;
    use clap::Parser;
    use rho_harness_core::config::Config;

    // Re-run via RHO_HOME env; serialize against other env-sensitive tests.
    // tokio Mutex: the guard is held across awaits by design.
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

    let config = Config::load(None).unwrap();
    config.ensure_dirs().unwrap();
    let mut session = crate::repl::ReplSession::new(config, AuthStore::default(), None)
        .with_cli(Some(Cli::parse_from(["rho", "--max-turns", "9"])));
    // Simulate a runtime /model selection made after startup.
    session.config.model = "runtime-model".to_string();
    let engine = crate::platform::agent_engine(session.config.clone(), session.auth_store.clone(), None)
        .await
        .unwrap();

    // The file changes after startup; /reload must pick it up.
    std::fs::write(home.join("config.toml"), "max_turns = 42\n").unwrap();

    let reloaded = session.reload_engine(&engine).await.unwrap();

    // Runtime model choice wins over config files...
    assert_eq!(reloaded.config.model, "runtime-model");
    // ...the engine adopts the re-read value...
    assert_eq!(reloaded.config.max_turns, 9, "CLI override beats the file");
    // ...and session config stays in sync with the engine.
    assert_eq!(session.config.max_turns, 9);
    assert_eq!(session.config.model, "runtime-model");

    unsafe {
        std::env::remove_var("RHO_HOME");
    }
    std::fs::remove_dir_all(home).unwrap();
}
