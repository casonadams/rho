use super::super::{Config, FileConfig, cli, merge};

#[test]
fn test_default_config() {
    let cfg = Config::default();
    assert!(!cfg.model.is_empty());
    assert_eq!(cfg.search_min_interval_ms, 2000);
    assert_eq!(cfg.output_max_bytes, 50_000);
    assert_eq!(cfg.max_output_tokens, None);
    assert_eq!(cfg.max_turns, 250);
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
        thinking: None,
        name: None,
        export: None,
        resume: None,
        r#continue: false,
        resume_picker: false,
        mode: "interactive".to_string(),
        message: Vec::new(),
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
fn test_positive_integer_parsing() {
    assert_eq!(merge::parse_positive_for_test::<usize>("LIMIT", "25").unwrap(), 25);
    assert!(merge::parse_positive_for_test::<usize>("LIMIT", "0").is_err());
    assert!(merge::parse_positive_for_test::<u64>("LIMIT", "invalid").is_err());
}
