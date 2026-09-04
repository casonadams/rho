#![cfg(unix)]

use rho_engine::auth::AuthStore;
use rho_engine::engine::builder::AgentEngineBuilder;
use rho_harness_core::config::{Config, McpConfig, McpServerConfig};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Instant;

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
async fn test_mcp_lazy_loading_non_blocking_startup_and_deferred_resolution() {
    with_dummy_provider_key();
    let workspace = temp_workspace();
    let server_script = workspace.join("delayed_mcp_server.sh");

    // Mock MCP server that sleeps 2s before responding to initialize
    let script_content = r#"#!/bin/sh
sleep 2.0
while IFS= read -r line; do
    if echo "$line" | grep -q '"method":"initialize"'; then
        id=$(echo "$line" | grep -o '"id":[0-9]*' | cut -d: -f2)
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{\"tools\":{}},\"serverInfo\":{\"name\":\"delayed-fs\",\"version\":\"1.0\"}}}"
    elif echo "$line" | grep -q '"method":"tools/list"'; then
        id=$(echo "$line" | grep -o '"id":[0-9]*' | cut -d: -f2)
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"tools\":[{\"name\":\"delayed_read\",\"description\":\"Read delayed\",\"inputSchema\":{\"type\":\"object\",\"properties\":{\"path\":{\"type\":\"string\"}},\"required\":[\"path\"]}}]}}"
    fi
done
"#;

    std::fs::write(&server_script, script_content).unwrap();
    std::fs::set_permissions(&server_script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut mcp_servers = BTreeMap::new();
    mcp_servers.insert(
        "delayed".to_string(),
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
        auto_approve: true,
        config_dir: workspace.clone(),
        sessions_dir: workspace.join("sessions"),
        auth_file: workspace.join("auth.json"),
        ..Config::default()
    };
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();

    // 1. Measure startup time: must be much faster than the 1s server sleep
    let start = Instant::now();
    let engine = AgentEngineBuilder::new(config, auth_store)
        .base_dir(workspace.clone())
        .build()
        .await
        .unwrap();
    let startup_elapsed = start.elapsed();

    // Startup should be virtually instant compared to the 2s server delay
    assert!(
        startup_elapsed.as_millis() < 1000,
        "Startup took {:?}, which indicates blocking on MCP server startup",
        startup_elapsed
    );

    // 2. Immediately after startup, only built-in tools should be registered
    let initial_tools = engine.tool_names();
    assert!(
        initial_tools.contains(&"read".to_string()),
        "built-in 'read' tool should be present immediately"
    );
    assert!(
        !initial_tools.contains(&"delayed_delayed_read".to_string()),
        "delayed MCP tool should not yet be in active tools list before resolution"
    );

    // 3. Deferred resolution: ensuring tools loaded resolves the background task
    engine.ensure_tools_loaded().await.unwrap();
    let resolved_tools = engine.tool_names();
    assert!(
        resolved_tools.contains(&"delayed_delayed_read".to_string()),
        "delayed MCP tool should be attached after ensure_tools_loaded, got: {resolved_tools:?}"
    );

    // 4. Subsequent calls to ensure_tools_loaded are instantaneous no-ops
    let second_start = Instant::now();
    engine.ensure_tools_loaded().await.unwrap();
    assert!(second_start.elapsed().as_millis() < 20);

    let _ = std::fs::remove_dir_all(&workspace);
}

#[tokio::test]
async fn test_mcp_lazy_loading_resilient_to_server_failure() {
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
        auto_approve: true,
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

    // Resolving failed servers must not fail the engine
    let res = engine.ensure_tools_loaded().await;
    assert!(
        res.is_ok(),
        "failed server should not cause ensure_tools_loaded error: {res:?}"
    );

    // Built-in tools must remain intact
    let tools = engine.tool_names();
    assert!(tools.contains(&"read".to_string()));
    assert!(tools.contains(&"bash".to_string()));

    let _ = std::fs::remove_dir_all(&workspace);
}
