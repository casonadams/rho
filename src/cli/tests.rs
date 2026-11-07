#[test]
fn rpc_request_roundtrip() {
    use rho_harness_core::rpc::protocol::{RpcCommand, RpcRequest};

    let prompt_req = RpcRequest {
        id: Some("1".to_string()),
        command: RpcCommand::Prompt {
            message: "Analyze repo".to_string(),
            images: None,
            streaming_behavior: Some("steer".to_string()),
        },
    };
    let json = serde_json::to_string(&prompt_req).unwrap();
    let deserialized: RpcRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.id, Some("1".to_string()));
    assert!(matches!(deserialized.command, RpcCommand::Prompt { ref message, .. } if message == "Analyze repo"));
}

#[test]
fn rpc_response_roundtrip() {
    use rho_harness_core::rpc::protocol::RpcResponse;

    let resp = RpcResponse::success(Some("1".to_string()), "prompt", None);
    let resp_json = serde_json::to_string(&resp).unwrap();
    assert!(resp_json.contains("\"success\":true"));
}

#[test]
fn test_cli_model_parsing() {
    use clap::Parser;
    use rho_harness_core::config::cli::Cli;

    let cli = Cli::try_parse_from(["rho", "--model", "anthropic/claude-3-7-sonnet"]).unwrap();
    assert_eq!(cli.model.as_deref(), Some("anthropic/claude-3-7-sonnet"));
    assert_eq!(cli.provider, None);

    let cli = Cli::try_parse_from(["rho", "--model", "openrouter/anthropic/claude-3.7-sonnet"]).unwrap();
    assert_eq!(cli.model.as_deref(), Some("openrouter/anthropic/claude-3.7-sonnet"));
    assert_eq!(cli.provider, None);

    let cli = Cli::try_parse_from(["rho", "-m", "local/qwen2.5-coder:7b"]).unwrap();
    assert_eq!(cli.model.as_deref(), Some("local/qwen2.5-coder:7b"));
    assert_eq!(cli.provider, None);

    let cli = Cli::try_parse_from(["rho", "--model", "claude-3-7-sonnet", "--provider", "anthropic"]).unwrap();
    assert_eq!(cli.model.as_deref(), Some("claude-3-7-sonnet"));
    assert_eq!(cli.provider.as_deref(), Some("anthropic"));
}

#[tokio::test]
async fn test_handle_command_mcp_lifecycle() {
    use super::commands::handle_command;
    use crate::auth::AuthStore;
    use crate::config::Config;
    use crate::config::cli::{Commands, McpCommands};
    use tempfile::tempdir;

    let temp = tempdir().unwrap();
    let config = Config {
        config_dir: temp.path().to_path_buf(),
        ..Default::default()
    };
    std::fs::write(config.config_dir.join("config.toml"), "").unwrap();
    let mut auth_store = AuthStore::load(temp.path().join("auth.json")).unwrap();

    let res = handle_command(
        Commands::Mcp {
            action: Some(McpCommands::List),
        },
        &config,
        &mut auth_store,
    )
    .await;
    assert!(res.is_ok());

    let res = handle_command(
        Commands::Mcp {
            action: Some(McpCommands::Add {
                name: "test_srv".to_string(),
                target: "echo".to_string(),
                args: vec!["hello".to_string()],
            }),
        },
        &config,
        &mut auth_store,
    )
    .await;
    assert!(res.is_ok());

    let res = handle_command(
        Commands::Mcp {
            action: Some(McpCommands::Remove {
                name: "test_srv".to_string(),
            }),
        },
        &config,
        &mut auth_store,
    )
    .await;
    assert!(res.is_ok());
}

#[tokio::test]
async fn test_handle_command_models_with_model_store() {
    use super::commands::{format_model_entries, handle_command};
    use crate::auth::AuthStore;
    use crate::config::Config;
    use crate::config::cli::Commands;
    use rho_engine::provider::{DiscoveredModel, ModelStore};
    use tempfile::tempdir;

    let temp = tempdir().unwrap();
    let config = Config {
        config_dir: temp.path().to_path_buf(),
        provider: "gemini".to_string(),
        model: "gemini-2.5-flash".to_string(),
        ..Default::default()
    };
    let mut store = ModelStore::load(config.config_dir.join("models-store.json"));
    store
        .set_models(
            "gemini",
            vec![DiscoveredModel {
                id: "gemini-3.6-flash".to_string(),
                name: "Gemini 3.6 Flash".to_string(),
                provider: "gemini".to_string(),
                description: "1M ctx · live test".to_string(),
                context_tokens: Some(1_000_000),
            }],
        )
        .unwrap();

    let mut auth_store = AuthStore::load(temp.path().join("auth.json")).unwrap();
    let res = handle_command(Commands::Models, &config, &mut auth_store).await;
    assert!(res.is_ok());

    let formatted = format_model_entries(store.get_models("gemini").unwrap(), &config.model);
    assert_eq!(formatted, vec!["  - gemini-3.6-flash (1M ctx · live test)"]);

    let empty_formatted = format_model_entries(&[], "fallback-model");
    assert_eq!(empty_formatted, vec!["  - fallback-model"]);
}

#[test]
fn test_format_config_summary_known_and_custom_provider() {
    use super::commands::format_config_summary;
    use crate::config::Config;
    use std::path::PathBuf;

    let config = Config {
        config_dir: PathBuf::from("/test/config/dir"),
        model: "claude-sonnet-4-6".to_string(),
        provider: "anthropic".to_string(),
        max_turns: 42,
        context_window_messages: 50,
        compaction_max_bytes: 100_000,
        ..Default::default()
    };
    let summary = format_config_summary(&config);
    assert_eq!(
        summary,
        vec![
            "Config location: /test/config/dir".to_string(),
            "Model: claude-sonnet-4-6".to_string(),
            "Provider: anthropic (API key)".to_string(),
            "Max turns: 42".to_string(),
            "Context window messages: 50".to_string(),
            "Compaction max bytes: 100000".to_string(),
        ]
    );

    let custom_config = Config {
        config_dir: PathBuf::from("/custom/dir"),
        model: "my-model".to_string(),
        provider: "unregistered_provider".to_string(),
        ..Default::default()
    };
    let custom_summary = format_config_summary(&custom_config);
    assert!(custom_summary.contains(&"Provider: unregistered_provider (custom)".to_string()));
}

#[tokio::test]
async fn test_handle_command_config_variants() {
    use super::commands::handle_command;
    use crate::auth::AuthStore;
    use crate::config::Config;
    use crate::config::cli::Commands;
    use tempfile::tempdir;

    let temp = tempdir().unwrap();
    let config = Config {
        config_dir: temp.path().to_path_buf(),
        model: "gpt-4o".to_string(),
        provider: "openai".to_string(),
        ..Default::default()
    };
    std::fs::write(config.config_dir.join("config.toml"), "").unwrap();
    let mut auth_store = AuthStore::load(temp.path().join("auth.json")).unwrap();

    let res = handle_command(Commands::Config { key: None, value: None }, &config, &mut auth_store).await;
    assert!(res.is_ok());

    let res = handle_command(
        Commands::Config {
            key: Some("model".to_string()),
            value: None,
        },
        &config,
        &mut auth_store,
    )
    .await;
    assert!(res.is_ok());

    let res = handle_command(
        Commands::Config {
            key: None,
            value: Some("val".to_string()),
        },
        &config,
        &mut auth_store,
    )
    .await;
    assert!(res.is_ok());

    let res = handle_command(
        Commands::Config {
            key: Some("model".to_string()),
            value: Some("custom-model".to_string()),
        },
        &config,
        &mut auth_store,
    )
    .await;
    assert!(res.is_ok());
    let saved = std::fs::read_to_string(config.config_dir.join("config.toml")).unwrap();
    assert!(saved.contains("custom-model"));
}
