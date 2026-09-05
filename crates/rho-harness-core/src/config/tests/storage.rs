use super::super::{Config, FileConfig, PluginConfig};

#[test]
fn test_state_file_loads_last_model_and_thinking_level() {
    let dir = std::env::temp_dir().join(format!("rho_config_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    crate::state::AppState::set_last_model(&dir, "gemini-2.0-flash", Some("gemini")).unwrap();
    crate::state::AppState::set_last_thinking_level(&dir, Some("high")).unwrap();

    let mut config = Config {
        config_dir: dir.clone(),
        ..Default::default()
    };

    let state = crate::state::AppState::load(&config.config_dir);
    if let Some(m) = state.last_model {
        config.model = m;
    }
    if let Some(p) = state.last_provider {
        config.provider = p;
    }
    if let Some(t) = state.last_thinking_level {
        config.thinking_level = Some(t);
    }

    assert_eq!(config.model, "gemini-2.0-flash");
    assert_eq!(config.provider, "gemini");
    assert_eq!(config.thinking_level.as_deref(), Some("high"));

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_set_file_value_persists_and_validates() {
    let dir = std::env::temp_dir().join(format!("rho_config_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    Config::set_file_value(&dir, "model", "gpt-test").unwrap();
    Config::set_file_value(&dir, "max_turns", "7").unwrap();
    Config::set_file_value(&dir, "theme", "nord").unwrap();
    let content = std::fs::read_to_string(dir.join("config.toml")).unwrap();
    let file: FileConfig = toml::from_str(&content).unwrap();
    assert_eq!(file.model.as_deref(), Some("gpt-test"));
    assert_eq!(file.max_turns, Some(7));
    assert_eq!(file.theme.as_deref(), Some("nord"));
    assert!(Config::set_file_value(&dir, "max_turns", "0").is_err());
    assert!(Config::set_file_value(&dir, "unknown", "value").is_err());

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn plugin_entries_round_trip_and_are_removed_atomically() {
    let dir = std::env::temp_dir().join(format!("rho_plugin_config_{}", uuid::Uuid::new_v4()));
    let plugin = PluginConfig {
        path: std::path::PathBuf::from("plugins/fixture"),
        package: Some("rho-plugin-fixture".to_string()),
        replaces: ["tool:bash".parse().unwrap()].into_iter().collect(),
        ..Default::default()
    };
    Config::add_plugin(&dir, "fixture", plugin.clone()).unwrap();
    let content = std::fs::read_to_string(dir.join("config.toml")).unwrap();
    let parsed: FileConfig = toml::from_str(&content).unwrap();
    assert_eq!(parsed.plugins.get("fixture"), Some(&plugin));
    assert_eq!(Config::remove_plugin(&dir, "fixture").unwrap(), plugin);
    let parsed: FileConfig = toml::from_str(&std::fs::read_to_string(dir.join("config.toml")).unwrap()).unwrap();
    assert!(parsed.plugins.is_empty());
    assert!(Config::remove_plugin(&dir, "fixture").is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_state_model_takes_precedence_over_config_file() {
    let dir = std::env::temp_dir().join(format!("rho_precedence_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    // 1. Write model to config.toml
    Config::set_file_value(&dir, "model", "config-model").unwrap();
    Config::set_file_value(&dir, "provider", "openai").unwrap();

    // 2. Write last used model to state.json
    crate::state::AppState::set_last_model(&dir, "state-model", Some("gemini")).unwrap();

    // 3. Load config pointing to dir
    unsafe {
        std::env::set_var("RHO_HOME", dir.to_str().unwrap());
    }
    let config = Config::load(None).unwrap();
    assert_eq!(config.model, "state-model");
    assert_eq!(config.provider, "gemini");
    assert!(config.model_from_state);

    // 4. Explicit CLI flag overrides state.json
    let cli = crate::config::cli::Cli {
        prompt: None,
        model: Some("cli-model".to_string()),
        provider: None,
        max_output_tokens: None,
        max_turns: None,
        thinking: None,
        name: None,
        export: None,
        resume: None,
        r#continue: false,
        resume_picker: false,
        mode: "interactive".to_string(),
        message: Vec::new(),
        system_prompt: None,
        append_system_prompt: None,
        no_context_files: false,
        command: None,
    };
    let cli_config = Config::load(Some(&cli)).unwrap();
    assert_eq!(cli_config.model, "cli-model");
    assert!(!cli_config.model_from_state);

    unsafe {
        std::env::remove_var("RHO_HOME");
    }
    std::fs::remove_dir_all(dir).unwrap();
}
