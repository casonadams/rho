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
