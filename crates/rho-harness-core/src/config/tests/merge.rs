use super::super::{Config, FileConfig, cli, merge};
use clap::Parser;

fn assert_default_limits(cfg: &Config) {
    let actual = (
        cfg.search_min_interval_ms,
        cfg.output_max_bytes,
        cfg.max_output_tokens,
        cfg.max_turns,
        cfg.context_window_messages,
        cfg.compaction_max_bytes,
    );
    assert_eq!(actual, (2000, 50_000, None, 1000, 24, 8192));
}

fn assert_default_features(cfg: &Config) {
    let actual = (
        !cfg.model.is_empty(),
        cfg.allow_private_network,
        cfg.session_retention_days,
        cfg.permission.enabled,
    );
    assert_eq!(actual, (true, false, Some(5), true));
}

#[test]
fn test_default_config() {
    let cfg = Config::default();
    assert_default_limits(&cfg);
    assert_default_features(&cfg);
}

fn assert_merged_models(cfg: &Config) {
    assert_eq!(cfg.model, "gpt-4o");
    assert_eq!(cfg.session_retention_days, Some(10));
    assert_eq!(cfg.provider, "openai");
}

fn assert_merged_limits(cfg: &Config) {
    let actual = (
        cfg.max_output_tokens,
        cfg.max_turns,
        cfg.context_limit,
        cfg.context_window_messages,
        cfg.compaction_max_bytes,
        cfg.search_min_interval_ms,
    );
    assert_eq!(actual, (Some(8192), 10, Some(65536), 16, 4096, 3000));
}

#[test]
fn test_file_merge() {
    let mut cfg = Config::default();
    let file_cfg = FileConfig {
        model: Some("gpt-4o".to_string()),
        provider: Some("openai".to_string()),
        max_output_tokens: Some(8192),
        max_turns: Some(10),
        context_limit: Some(65536),
        context_window_messages: Some(16),
        compaction_max_bytes: Some(4096),
        search_min_interval_ms: Some(3000),
        session_retention_days: Some(10),
        ..Default::default()
    };
    merge::merge_file(&mut cfg, file_cfg);
    assert_merged_models(&cfg);
    assert_merged_limits(&cfg);
}

fn sample_cli_override(model: &str, turns: usize) -> cli::Cli {
    cli::Cli {
        prompt: None,
        model: Some(model.to_string()),
        provider: None,
        max_output_tokens: None,
        max_turns: Some(turns),
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
    }
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

    let cli = sample_cli_override("cli-model", 40);
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

    let environment = std::collections::HashMap::from([("WEB_ALLOW_PRIVATE_NETWORK", "sometimes")]);
    let error = merge::apply_env_overrides_with(&mut config, |name| {
        environment.get(name).map(|value| (*value).to_string())
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("WEB_ALLOW_PRIVATE_NETWORK"));
}

#[test]
fn test_positive_integer_parsing() {
    assert_eq!(merge::parse_positive_for_test::<usize>("LIMIT", "25").unwrap(), 25);
    assert!(merge::parse_positive_for_test::<usize>("LIMIT", "0").is_err());
    assert!(merge::parse_positive_for_test::<u64>("LIMIT", "invalid").is_err());
}

#[test]
fn test_cli_context_flag_overrides() {
    let mut config = Config::default();
    let cli = cli::Cli::try_parse_from([
        "rho",
        "--system-prompt",
        "custom system prompt",
        "--append-system-prompt",
        "additional instructions",
        "--nc",
    ])
    .unwrap();
    merge::apply_cli_overrides(&mut config, Some(&cli));

    assert_eq!(config.system_prompt.as_deref(), Some("custom system prompt"));
    assert_eq!(config.append_system_prompt.as_deref(), Some("additional instructions"));
    assert!(config.no_context_files);
}

#[test]
fn test_cli_permission_flag_override() {
    let mut config = Config::default();
    assert!(config.permission.enabled);
    let cli = cli::Cli::try_parse_from(["rho", "--no-permission"]).unwrap();
    merge::apply_cli_overrides(&mut config, Some(&cli));
    assert!(!config.permission.enabled);
}
