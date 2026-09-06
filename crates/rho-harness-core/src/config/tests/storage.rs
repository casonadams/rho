use super::super::{Config, FileConfig, PluginConfig};

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn test_config_file_loads_model_provider_and_thinking_level() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = std::env::temp_dir().join(format!("rho_cfg_load_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    Config::set_file_value(&dir, "model", "gemini-2.0-flash").unwrap();
    Config::set_file_value(&dir, "provider", "gemini").unwrap();
    Config::set_file_value(&dir, "thinking_level", "high").unwrap();

    unsafe {
        std::env::set_var("RHO_HOME", dir.to_str().unwrap());
    }
    let config = Config::load(None).unwrap();

    assert_eq!(config.model, "gemini-2.0-flash");
    assert_eq!(config.provider, "gemini");
    assert_eq!(config.thinking_level.as_deref(), Some("high"));
    assert_eq!(config.default_model.as_deref(), Some("gemini-2.0-flash"));
    assert_eq!(config.default_provider.as_deref(), Some("gemini"));

    unsafe {
        std::env::remove_var("RHO_HOME");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_config_file_aliases_load_correctly() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = std::env::temp_dir().join(format!("rho_cfg_alias_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    let toml = r#"
default_model = "claude-3-7-sonnet-20250219"
default_provider = "anthropic"
default_thinking = "medium"
"#;
    std::fs::write(dir.join("config.toml"), toml).unwrap();

    unsafe {
        std::env::set_var("RHO_HOME", dir.to_str().unwrap());
    }
    let config = Config::load(None).unwrap();

    assert_eq!(config.model, "claude-3-7-sonnet-20250219");
    assert_eq!(config.provider, "anthropic");
    assert_eq!(config.thinking_level.as_deref(), Some("medium"));
    assert_eq!(config.default_model.as_deref(), Some("claude-3-7-sonnet-20250219"));
    assert_eq!(config.default_provider.as_deref(), Some("anthropic"));

    unsafe {
        std::env::remove_var("RHO_HOME");
    }
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
fn test_cli_overrides_config_file() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = std::env::temp_dir().join(format!("rho_precedence_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    Config::set_file_value(&dir, "model", "config-model").unwrap();
    Config::set_file_value(&dir, "provider", "openai").unwrap();

    unsafe {
        std::env::set_var("RHO_HOME", dir.to_str().unwrap());
    }
    let config = Config::load(None).unwrap();
    assert_eq!(config.model, "config-model");
    assert_eq!(config.provider, "openai");

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
        no_permission: false,
        command: None,
    };
    let cli_config = Config::load(Some(&cli)).unwrap();
    assert_eq!(cli_config.model, "cli-model");

    unsafe {
        std::env::remove_var("RHO_HOME");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn test_save_default_model_persists_both_fields() {
    let dir = std::env::temp_dir().join(format!("rho_save_default_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    Config::save_default_model_async(&dir, "saved-model", "saved-provider")
        .await
        .unwrap();

    let (model, provider, default_model, default_provider) = {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("RHO_HOME", dir.to_str().unwrap());
        }
        let config = Config::load(None).unwrap();
        unsafe {
            std::env::remove_var("RHO_HOME");
        }
        (
            config.model,
            config.provider,
            config.default_model,
            config.default_provider,
        )
    };
    assert_eq!(model, "saved-model");
    assert_eq!(provider, "saved-provider");
    assert_eq!(default_model.as_deref(), Some("saved-model"));
    assert_eq!(default_provider.as_deref(), Some("saved-provider"));

    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn test_save_default_thinking_level_persists() {
    let dir = std::env::temp_dir().join(format!("rho_save_thinking_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    Config::save_default_thinking_level_async(&dir, Some("high"))
        .await
        .unwrap();

    let thinking_level = {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("RHO_HOME", dir.to_str().unwrap());
        }
        let config = Config::load(None).unwrap();
        unsafe {
            std::env::remove_var("RHO_HOME");
        }
        config.thinking_level
    };
    assert_eq!(thinking_level.as_deref(), Some("high"));

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_config_file_model_infers_provider_when_unspecified() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = std::env::temp_dir().join(format!("rho_infer_cfg_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    Config::set_file_value(&dir, "model", "gemini-2.0-flash").unwrap();

    unsafe {
        std::env::set_var("RHO_HOME", dir.to_str().unwrap());
    }
    let config = Config::load(None).unwrap();
    assert_eq!(config.model, "gemini-2.0-flash");
    assert_eq!(config.provider, "gemini");

    unsafe {
        std::env::remove_var("RHO_HOME");
    }
    std::fs::remove_dir_all(dir).unwrap();
}
