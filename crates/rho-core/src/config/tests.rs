use super::*;

#[test]
fn test_set_file_value_persists_and_validates() {
    let dir = std::env::temp_dir().join(format!("rho_config_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    Config::set_file_value(&dir, "model", "gpt-test").unwrap();
    Config::set_file_value(&dir, "max_turns", "7").unwrap();
    let content = std::fs::read_to_string(dir.join("config.toml")).unwrap();
    let file: FileConfig = toml::from_str(&content).unwrap();
    assert_eq!(file.model.as_deref(), Some("gpt-test"));
    assert_eq!(file.max_turns, Some(7));
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
fn rejects_invalid_plugin_configuration() {
    let mut config = Config::default();
    config.plugins.insert(
        "Invalid Name".to_string(),
        PluginConfig {
            path: "plugin".into(),
            package: None,
            version: None,
            git: None,
            branch: None,
            tag: None,
            enabled: true,
            replaces: Default::default(),
            config: None,
        },
    );
    assert!(config.validate().is_err());
}

#[test]
fn parses_cargo_style_plugins_and_mcp_config() {
    let toml_str = r#"
model = "gpt-4"

[plugins.local_tool]
path = "./tools/my_tool"
enabled = true

[plugins.git_tool]
git = "https://github.com/org/plugin"
branch = "main"

[plugins.crate_tool]
package = "rho-plugin-review"
version = "0.1.0"

[mcp]
enabled = true

[mcp.servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/workspace"]
env = { DEBUG = "true" }
enabled = true

[mcp.servers.linear]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-linear"]
"#;
    let file: FileConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(file.plugins.len(), 3);
    assert_eq!(
        file.plugins["local_tool"].path,
        std::path::PathBuf::from("./tools/my_tool")
    );
    assert!(file.plugins["local_tool"].enabled);
    assert_eq!(
        file.plugins["git_tool"].git.as_deref(),
        Some("https://github.com/org/plugin")
    );
    assert_eq!(file.plugins["crate_tool"].package.as_deref(), Some("rho-plugin-review"));

    let mcp = file.mcp.unwrap();
    assert!(mcp.enabled);
    assert_eq!(mcp.servers.len(), 2);
    assert_eq!(mcp.servers["filesystem"].command, "npx");
    assert_eq!(
        mcp.servers["filesystem"].args,
        vec!["-y", "@modelcontextprotocol/server-filesystem", "/workspace"]
    );
    assert_eq!(
        mcp.servers["filesystem"].env.get("DEBUG").map(|s| s.as_str()),
        Some("true")
    );
    assert_eq!(mcp.servers["linear"].command, "npx");
}

#[test]
fn test_default_config() {
    let cfg = Config::default();
    assert!(!cfg.model.is_empty());
    assert_eq!(cfg.search_min_interval_ms, 2000);
    assert_eq!(cfg.output_max_bytes, 50_000);
    assert_eq!(cfg.max_output_tokens, None);
    assert_eq!(cfg.max_turns, 100);
    assert_eq!(cfg.context_window_messages, 24);
    assert_eq!(cfg.compaction_max_bytes, 8192);
    assert!(!cfg.allow_private_network);
    assert!(cfg.plugins.is_empty());
}

#[test]
fn test_file_merge() {
    let mut cfg = Config::default();
    let file_cfg = FileConfig {
        model: Some("gpt-4o".to_string()),
        provider: Some("openai".to_string()),
        auto_approve: Some(true),
        max_output_tokens: Some(8192),
        max_turns: Some(10),
        context_limit: Some(65536),
        context_window_messages: Some(16),
        compaction_max_bytes: Some(4096),
        search_min_interval_ms: Some(3000),
        ..Default::default()
    };
    merge::merge_file(&mut cfg, file_cfg);
    assert_eq!(cfg.model, "gpt-4o");
    assert_eq!(cfg.provider, "openai");
    assert!(cfg.auto_approve);
    assert_eq!(cfg.max_output_tokens, Some(8192));
    assert_eq!(cfg.max_turns, 10);
    assert_eq!(cfg.context_limit, Some(65536));
    assert_eq!(cfg.context_window_messages, 16);
    assert_eq!(cfg.compaction_max_bytes, 4096);
    assert_eq!(cfg.search_min_interval_ms, 3000);
}

#[test]
fn test_precedence_is_defaults_file_environment_then_cli() {
    let mut config = Config::default();
    merge::merge_file(
        &mut config,
        FileConfig {
            model: Some("file-model".to_string()),
            max_turns: Some(20),
            ..Default::default()
        },
    );

    let environment = std::collections::HashMap::from([("AI_MODEL", "environment-model"), ("AI_MAX_TURNS", "30")]);
    merge::apply_env_overrides_with(&mut config, |name| {
        environment.get(name).map(|value| (*value).to_string())
    })
    .unwrap();

    let cli = cli::Cli {
        prompt: None,
        model: Some("cli-model".to_string()),
        provider: None,
        max_output_tokens: None,
        max_turns: Some(40),
        auto_approve: false,
        resume: None,
        r#continue: false,
        resume_picker: false,
        mode: "interactive".to_string(),
        command: None,
    };
    merge::apply_cli_overrides(&mut config, Some(&cli));

    assert_eq!(config.model, "cli-model");
    assert_eq!(config.max_turns, 40);
}

#[test]
fn test_invalid_environment_values_are_rejected() {
    let mut config = Config::default();
    let environment = std::collections::HashMap::from([("AI_CONTEXT_LIMIT", "not-a-number")]);
    let error = merge::apply_env_overrides_with(&mut config, |name| {
        environment.get(name).map(|value| (*value).to_string())
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("AI_CONTEXT_LIMIT"));

    let environment = std::collections::HashMap::from([("AI_AUTO_APPROVE", "sometimes")]);
    let error = merge::apply_env_overrides_with(&mut config, |name| {
        environment.get(name).map(|value| (*value).to_string())
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("AI_AUTO_APPROVE"));
}

#[test]
fn test_runtime_limit_boundaries() {
    let mut cfg = Config {
        max_turns: 0,
        ..Config::default()
    };
    assert!(cfg.validate().is_err());

    cfg.max_turns = 1;
    cfg.max_output_tokens = Some(0);
    assert!(cfg.validate().is_err());

    cfg.max_output_tokens = Some(1);
    cfg.context_window_messages = 0;
    assert!(cfg.validate().is_err());

    cfg.context_window_messages = 1;
    cfg.compaction_max_bytes = 0;
    assert!(cfg.validate().is_err());

    cfg.compaction_max_bytes = 1;
    assert!(cfg.validate().is_ok());
}

#[test]
fn test_positive_integer_parsing() {
    assert_eq!(merge::parse_positive_for_test::<usize>("LIMIT", "25").unwrap(), 25);
    assert!(merge::parse_positive_for_test::<usize>("LIMIT", "0").is_err());
    assert!(merge::parse_positive_for_test::<u64>("LIMIT", "invalid").is_err());
}
