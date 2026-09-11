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

fn config_with_dup_plugin(dir: &std::path::Path) -> crate::config::Config {
    use rho_harness_core::config::PluginConfig;
    let mut plugins = std::collections::BTreeMap::new();
    plugins.insert(
        "rho-plugin-dup".to_string(),
        PluginConfig {
            command: Some("rho-plugin-dup".to_string()),
            ..Default::default()
        },
    );
    crate::config::Config {
        config_dir: dir.to_path_buf(),
        plugins,
        ..Default::default()
    }
}

#[tokio::test]
async fn test_handle_command_install_duplicate_error() {
    use super::commands::handle_command;
    use crate::auth::AuthStore;
    use crate::config::cli::Commands;
    use tempfile::tempdir;

    let temp = tempdir().unwrap();
    let config = config_with_dup_plugin(temp.path());
    let mut auth_store = AuthStore::load(temp.path().join("auth.json")).unwrap();
    let res = handle_command(
        Commands::Install {
            target: "dup".to_string(),
            force: false,
        },
        &config,
        &mut auth_store,
    )
    .await;
    assert!(res.is_err());
}

#[tokio::test]
async fn test_handle_command_plugin_install_duplicate_error() {
    use super::commands::handle_command;
    use crate::auth::AuthStore;
    use crate::config::cli::{Commands, PluginCommands};
    use tempfile::tempdir;

    let temp = tempdir().unwrap();
    let config = config_with_dup_plugin(temp.path());
    let mut auth_store = AuthStore::load(temp.path().join("auth.json")).unwrap();
    let res = handle_command(
        Commands::Plugin {
            action: Some(PluginCommands::Install {
                target: "dup".to_string(),
                force: false,
            }),
        },
        &config,
        &mut auth_store,
    )
    .await;
    assert!(res.is_err());
}

#[tokio::test]
async fn test_handle_command_update_plugin_missing() {
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
    let mut auth_store = AuthStore::load(temp.path().join("auth.json")).unwrap();
    let res = handle_command(
        Commands::Update {
            target: Some("nonexistent".to_string()),
        },
        &config,
        &mut auth_store,
    )
    .await;
    assert!(res.is_err());
}

#[tokio::test]
async fn test_handle_command_update_all_empty() {
    use super::commands::handle_command;
    use crate::auth::AuthStore;
    use crate::config::Config;
    use crate::config::cli::{Commands, PluginCommands};
    use tempfile::tempdir;

    let temp = tempdir().unwrap();
    let config = Config {
        config_dir: temp.path().to_path_buf(),
        ..Default::default()
    };
    let mut auth_store = AuthStore::load(temp.path().join("auth.json")).unwrap();
    let res = handle_command(
        Commands::Plugin {
            action: Some(PluginCommands::Update { target: None }),
        },
        &config,
        &mut auth_store,
    )
    .await;
    assert!(res.is_ok());
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
