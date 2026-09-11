#![cfg(unix)]

use rho_engine::auth::AuthStore;
use rho_engine::engine::builder::AgentEngineBuilder;
use rho_harness_core::config::{Config, McpConfig, McpServerConfig};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn with_dummy_provider_key() {
    unsafe {
        std::env::set_var("ANTHROPIC_API_KEY", "test-key-not-real");
    }
}

fn temp_workspace() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mcp_lazy_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_fast_mcp_server(workspace: &std::path::Path) -> PathBuf {
    let script_path = workspace.join("fast_mcp_server.sh");
    let script = "#!/bin/sh\nwhile IFS= read -r line; do\n  id=$(echo \"$line\" | grep -o '\"id\":[0-9]*' | cut -d: -f2)\n  case \"$line\" in\n    *\"initialize\"*) echo \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":$id,\\\"result\\\":{\\\"protocolVersion\\\":\\\"2024-11-05\\\",\\\"capabilities\\\":{\\\"tools\\\":{}},\\\"serverInfo\\\":{\\\"name\\\":\\\"fast-fs\\\",\\\"version\\\":\\\"1.0\\\"}}}\" ;;\n    *\"tools/list\"*) echo \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":$id,\\\"result\\\":{\\\"tools\\\":[{\\\"name\\\":\\\"fast_read\\\",\\\"inputSchema\\\":{}}]}}\" ;;\n  esac\ndone\n";
    std::fs::write(&script_path, script).unwrap();
    std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    script_path
}

fn build_mcp_test_config(workspace: &std::path::Path, server_name: &str, cmd: &str) -> Config {
    let mut servers = BTreeMap::new();
    servers.insert(server_name.to_string(), McpServerConfig::stdio(cmd, Vec::new()));
    Config {
        mcp: McpConfig { enabled: true, servers },
        config_dir: workspace.to_path_buf(),
        sessions_dir: workspace.join("sessions"),
        auth_file: workspace.join("auth.json"),
        ..Config::default()
    }
}

#[tokio::test]
async fn test_mcp_eager_loading_attaches_tools_before_first_turn() {
    with_dummy_provider_key();
    let workspace = temp_workspace();
    let script = write_fast_mcp_server(&workspace);
    let config = build_mcp_test_config(&workspace, "fast", script.to_str().unwrap());
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();

    let engine = AgentEngineBuilder::new(config, auth_store)
        .base_dir(workspace.clone())
        .build()
        .await
        .unwrap();
    let tools = engine.tool_names();
    assert!(tools.contains(&"read".to_string()) && tools.contains(&"fast_fast_read".to_string()));
    let _ = std::fs::remove_dir_all(&workspace);
}

#[tokio::test]
async fn test_mcp_eager_loading_resilient_to_server_failure() {
    let workspace = temp_workspace();
    let config = build_mcp_test_config(&workspace, "broken", "/nonexistent/binary");
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();

    let engine = AgentEngineBuilder::new(config, auth_store)
        .base_dir(workspace.clone())
        .build()
        .await
        .unwrap();
    let tools = engine.tool_names();
    assert!(tools.contains(&"read".to_string()) && tools.contains(&"bash".to_string()));
    let _ = std::fs::remove_dir_all(&workspace);
}
