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

#[tokio::test]
async fn test_mcp_eager_loading_attaches_tools_before_first_turn() {
    with_dummy_provider_key();
    let workspace = temp_workspace();
    let server_script = workspace.join("fast_mcp_server.sh");

    let script_content = r#"#!/bin/sh
while IFS= read -r line; do
    if echo "$line" | grep -q '"method":"initialize"'; then
        id=$(echo "$line" | grep -o '"id":[0-9]*' | cut -d: -f2)
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{\"tools\":{}},\"serverInfo\":{\"name\":\"fast-fs\",\"version\":\"1.0\"}}}"
    elif echo "$line" | grep -q '"method":"tools/list"'; then
        id=$(echo "$line" | grep -o '"id":[0-9]*' | cut -d: -f2)
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"tools\":[{\"name\":\"fast_read\",\"description\":\"Read fast\",\"inputSchema\":{\"type\":\"object\",\"properties\":{\"path\":{\"type\":\"string\"}},\"required\":[\"path\"]}}]}}"
    fi
done
"#;

    std::fs::write(&server_script, script_content).unwrap();
    std::fs::set_permissions(&server_script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut mcp_servers = BTreeMap::new();
    mcp_servers.insert(
        "fast".to_string(),
        McpServerConfig {
            command: server_script.to_str().unwrap().to_string(),
            args: Vec::new(),
            env: BTreeMap::new(),
            enabled: true,
        },
    );

    let config = Config {
        mcp: McpConfig {
            enabled: true,
            servers: mcp_servers,
        },
        config_dir: workspace.clone(),
        sessions_dir: workspace.join("sessions"),
        auth_file: workspace.join("auth.json"),
        ..Config::default()
    };
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();

    // 1. Build engine eagerly
    let engine = AgentEngineBuilder::new(config, auth_store)
        .base_dir(workspace.clone())
        .build()
        .await
        .unwrap();

    // 2. Immediately after build, both built-in and MCP tools must be ready
    let tools = engine.tool_names();
    assert!(
        tools.contains(&"read".to_string()),
        "built-in 'read' tool should be present immediately"
    );
    assert!(
        tools.contains(&"fast_fast_read".to_string()),
        "MCP tool should be attached eagerly on engine build, got: {tools:?}"
    );

    let _ = std::fs::remove_dir_all(&workspace);
}

#[tokio::test]
async fn test_mcp_eager_loading_resilient_to_server_failure() {
    let workspace = temp_workspace();

    let mut mcp_servers = BTreeMap::new();
    mcp_servers.insert(
        "broken".to_string(),
        McpServerConfig {
            command: "/nonexistent/binary/that/cannot/be/spawned".to_string(),
            args: Vec::new(),
            env: BTreeMap::new(),
            enabled: true,
        },
    );

    let config = Config {
        mcp: McpConfig {
            enabled: true,
            servers: mcp_servers,
        },
        config_dir: workspace.clone(),
        sessions_dir: workspace.join("sessions"),
        auth_file: workspace.join("auth.json"),
        ..Config::default()
    };
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();

    let engine = AgentEngineBuilder::new(config, auth_store)
        .base_dir(workspace.clone())
        .build()
        .await
        .unwrap();

    // Built-in tools must remain intact even if MCP server failed
    let tools = engine.tool_names();
    assert!(tools.contains(&"read".to_string()));
    assert!(tools.contains(&"bash".to_string()));

    let _ = std::fs::remove_dir_all(&workspace);
}
