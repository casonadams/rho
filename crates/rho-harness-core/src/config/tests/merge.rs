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

#[test]
fn test_cli_provider_switch_picks_model_from_models_table() {
    let mut config = Config::default();
    merge::merge_file(
        &mut config,
        FileConfig {
            model: Some("claude-sonnet-5".to_string()),
            provider: Some("claude".to_string()),
            models: std::collections::BTreeMap::from([
                ("claude".to_string(), "claude-sonnet-5".to_string()),
                ("gemini".to_string(), "gemini-3.6-flash".to_string()),
            ]),
            ..Default::default()
        },
    );
    assert_eq!(config.provider, "claude");
    assert_eq!(config.model, "claude-sonnet-5");

    let cli = cli::Cli::try_parse_from(["rho", "--provider", "gemini"]).unwrap();
    merge::apply_cli_overrides(&mut config, Some(&cli));

    assert_eq!(config.provider, "gemini");
    assert_eq!(config.model, "gemini-3.6-flash");
}

#[test]
fn test_cli_provider_switch_falls_back_to_canonical_default_when_not_in_models_table() {
    let mut config = Config::default();
    merge::merge_file(
        &mut config,
        FileConfig {
            model: Some("claude-sonnet-5".to_string()),
            provider: Some("claude".to_string()),
            ..Default::default()
        },
    );

    let cli = cli::Cli::try_parse_from(["rho", "--provider", "groq"]).unwrap();
    merge::apply_cli_overrides(&mut config, Some(&cli));

    assert_eq!(config.provider, "groq");
    assert_eq!(config.model, "llama-3.3-70b-versatile");
}

#[test]
fn test_cli_provider_switch_with_explicit_model_flag_overrides_models_table() {
    let mut config = Config::default();
    merge::merge_file(
        &mut config,
        FileConfig {
            model: Some("claude-sonnet-5".to_string()),
            provider: Some("claude".to_string()),
            models: std::collections::BTreeMap::from([("gemini".to_string(), "gemini-3.6-flash".to_string())]),
            ..Default::default()
        },
    );

    let cli = cli::Cli::try_parse_from(["rho", "--provider", "gemini", "-m", "gemini-custom"]).unwrap();
    merge::apply_cli_overrides(&mut config, Some(&cli));

    assert_eq!(config.provider, "gemini");
    assert_eq!(config.model, "gemini-custom");
}

#[test]
fn test_tools_web_search_default_and_merge() {
    let cfg = Config::default();
    assert_eq!(cfg.tools.web.search.default, "brave");
    assert_eq!(cfg.tools.web.search.fallback, vec!["duckduckgo", "yahoo"]);

    let toml_str = r#"
        [tools.web.search]
        default = "duckduckgo"
        fallback = ["yahoo", "firecrawl"]
    "#;
    let file_cfg: FileConfig = toml::from_str(toml_str).unwrap();
    let mut merged = Config::default();
    merge::merge_file(&mut merged, file_cfg);

    assert_eq!(merged.tools.web.search.default, "duckduckgo");
    assert_eq!(merged.tools.web.search.fallback, vec!["yahoo", "firecrawl"]);
}

#[tokio::test]
async fn test_legacy_model_config_migration() {
    // 1. model_provider + model -> <provider>/<model> with warning
    let toml1 = r#"
        model_provider = "openai"
        model = "gpt-4o"
    "#;
    let file_cfg1: FileConfig = toml::from_str(toml1).unwrap();
    let mut config1 = Config::default();
    merge::merge_file(&mut config1, file_cfg1);
    assert_eq!(config1.model, "openai/gpt-4o");
    assert_eq!(config1.provider, "openai");
    assert!(config1.migration_warnings.iter().any(|w| w.contains("deprecated")));

    // 2. providers.anthropic.default_model -> anthropic/<default_model> with warning
    let toml2 = r#"
        [providers.anthropic]
        default_model = "claude-3-7-sonnet"
    "#;
    let file_cfg2: FileConfig = toml::from_str(toml2).unwrap();
    let mut config2 = Config::default();
    merge::merge_file(&mut config2, file_cfg2);
    assert_eq!(config2.model, "anthropic/claude-3-7-sonnet");
    assert_eq!(config2.provider, "anthropic");
    assert!(config2.validate().is_ok());
    assert!(config2.migration_warnings.iter().any(|w| w.contains("deprecated")));

    // 3. Custom provider with base_url and default_model
    let toml3 = r#"
        [providers.custom-llm]
        base_url = "https://custom.ai/v1"
        default_model = "model-x"
    "#;
    let file_cfg3: FileConfig = toml::from_str(toml3).unwrap();
    let mut config3 = Config::default();
    merge::merge_file(&mut config3, file_cfg3);
    assert_eq!(config3.model, "custom-llm/model-x");
    assert_eq!(config3.provider, "custom-llm");
    assert!(config3.providers.contains_key("custom-llm"));
    assert!(config3.validate().is_ok());

    // 4. Canonical format emits no warnings
    let toml4 = r#"
        model = "anthropic/claude-3-7-sonnet"
    "#;
    let file_cfg4: FileConfig = toml::from_str(toml4).unwrap();
    let mut config4 = Config::default();
    merge::merge_file(&mut config4, file_cfg4);
    assert_eq!(config4.model, "anthropic/claude-3-7-sonnet");
    assert_eq!(config4.provider, "anthropic");
    assert!(config4.migration_warnings.is_empty());

    // 5. Saving config does not write deprecated fields
    let dir = std::env::temp_dir().join(format!("rho_test_mig_save_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let legacy_toml = "model_provider = \"openai\"\n[providers.openai]\ndefault_model = \"gpt-4o\"\n";
    std::fs::write(dir.join("config.toml"), legacy_toml).unwrap();

    Config::save_default_model_async(&dir, "gpt-4o", "openai")
        .await
        .unwrap();

    let saved = std::fs::read_to_string(dir.join("config.toml")).unwrap();
    assert!(!saved.contains("model_provider"));
    assert!(!saved.contains("default_model"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_canonical_model_spec() {
    let mut config = Config {
        model: "anthropic/claude-3-7-sonnet".to_string(),
        provider: "anthropic".to_string(),
        ..Default::default()
    };
    assert_eq!(config.canonical_model_spec(), "anthropic/claude-3-7-sonnet");

    config.model = "openrouter/anthropic/claude-3.7-sonnet".to_string();
    config.provider = "openrouter".to_string();
    assert_eq!(config.canonical_model_spec(), "openrouter/anthropic/claude-3.7-sonnet");

    config.model = "gpt-4o".to_string();
    config.provider = "openai".to_string();
    assert_eq!(config.canonical_model_spec(), "openai/gpt-4o");

    config.model = "claude-3-7-sonnet".to_string();
    config.provider = String::new();
    assert_eq!(config.canonical_model_spec(), "anthropic/claude-3-7-sonnet");

    config.model = "qwen2.5-coder:7b".to_string();
    config.provider = String::new();
    assert_eq!(config.canonical_model_spec(), "local/qwen2.5-coder:7b");

    config.model = String::new();
    config.provider = "openai".to_string();
    assert_eq!(config.canonical_model_spec(), "openai/gpt-4o");

    config.model = String::new();
    config.provider = "local".to_string();
    assert_eq!(config.canonical_model_spec(), "local/llama3.2:latest");
}

#[test]
fn test_cli_override_model_resolution() {
    use crate::config::cli::Cli;
    use clap::Parser;

    let cases = [
        (
            vec!["rho", "--model", "anthropic/claude-3-7-sonnet"],
            "anthropic",
            "anthropic/claude-3-7-sonnet",
            false,
        ),
        (
            vec!["rho", "--model", "openrouter/anthropic/claude-3.7-sonnet"],
            "openrouter",
            "openrouter/anthropic/claude-3.7-sonnet",
            false,
        ),
        (
            vec!["rho", "--model", "claude-3-7-sonnet"],
            "anthropic",
            "claude-3-7-sonnet",
            false,
        ),
        (vec!["rho", "--model", "custom-model"], "local", "custom-model", false),
        (
            vec![
                "rho",
                "--model",
                "anthropic/claude-3-7-sonnet",
                "--provider",
                "anthropic",
            ],
            "anthropic",
            "anthropic/claude-3-7-sonnet",
            false,
        ),
        (
            vec!["rho", "--model", "anthropic/claude-3-7-sonnet", "--provider", "openai"],
            "anthropic",
            "anthropic/claude-3-7-sonnet",
            true,
        ),
        (
            vec!["rho", "--model", "gpt-4o", "--provider", "openai"],
            "openai",
            "gpt-4o",
            false,
        ),
        (vec!["rho", "--provider", "openai"], "openai", "gpt-4o", false),
    ];

    for (args, expected_p, expected_m, expect_warn) in cases {
        let mut config = Config::default();
        let cli = Cli::try_parse_from(args).unwrap();
        merge::apply_cli_overrides(&mut config, Some(&cli));
        assert_eq!(config.provider, expected_p);
        assert_eq!(config.model, expected_m);
        if expect_warn {
            assert!(config.migration_warnings.iter().any(|w| w.contains(
                "Warning: Provider 'openai' overridden by provider in model spec 'anthropic'."
            )));
        } else {
            assert!(config.migration_warnings.is_empty());
        }
    }
}

#[test]
fn test_env_override_model_resolution() {
    let mut config = Config::default();
    crate::config::merge::apply_env_overrides_with(&mut config, |name| match name {
        "RHO_MODEL" => Some("openrouter/meta-llama/llama-3.3-70b-instruct:free".to_string()),
        _ => None,
    })
    .unwrap();
    assert_eq!(config.provider, "openrouter");
    assert_eq!(config.model, "openrouter/meta-llama/llama-3.3-70b-instruct:free");

    let mut config = Config::default();
    crate::config::merge::apply_env_overrides_with(&mut config, |name| match name {
        "RHO_MODEL" => Some("anthropic/claude-3-7-sonnet".to_string()),
        "RHO_PROVIDER" => Some("openai".to_string()),
        _ => None,
    })
    .unwrap();
    assert_eq!(config.provider, "anthropic");
    assert_eq!(config.model, "anthropic/claude-3-7-sonnet");
    assert!(
        config
            .migration_warnings
            .iter()
            .any(|w| w.contains("Warning: Provider 'openai' overridden by provider in model spec 'anthropic'."))
    );
}
