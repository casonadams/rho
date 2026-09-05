#[test]
fn rpc_request_and_response_roundtrip() {
    use rho_harness_core::rpc::protocol::{RpcCommand, RpcRequest, RpcResponse};

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

    let resp = RpcResponse::success(Some("1".to_string()), "prompt", None);
    let resp_json = serde_json::to_string(&resp).unwrap();
    assert!(resp_json.contains("\"success\":true"));
}

#[tokio::test]
async fn test_handle_command_plugin_list() {
    use super::commands::handle_command;
    use crate::auth::AuthStore;
    use crate::config::Config;
    use crate::config::cli::{Commands, PluginCommands};

    let temp = tempfile::tempdir().unwrap();
    let config = Config::default();
    let mut auth_store = AuthStore::load(temp.path().join("auth.json")).unwrap();
    let res = handle_command(
        Commands::Plugin {
            action: Some(PluginCommands::List),
        },
        &config,
        &mut auth_store,
    )
    .await;
    assert!(res.is_ok());
}

#[tokio::test]
async fn test_handle_command_remove_missing() {
    use super::commands::handle_command;
    use crate::auth::AuthStore;
    use crate::config::Config;
    use crate::config::cli::Commands;
    use tempfile::tempdir;

    let temp = tempdir().unwrap();
    let config = Config {
        config_dir: temp.path().to_path_buf(),
        ..Default::default()
    };
    std::fs::write(config.config_dir.join("config.toml"), "").unwrap();

    let mut auth_store = AuthStore::load(temp.path().join("auth.json")).unwrap();
    let res = handle_command(
        Commands::Remove {
            name: "nonexistent".to_string(),
            keep_binary: false,
        },
        &config,
        &mut auth_store,
    )
    .await;
    assert!(res.is_err());
}
