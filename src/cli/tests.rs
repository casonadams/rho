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
