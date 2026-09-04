#![cfg(unix)]

use rho_engine::mcp::load_mcp_tools;
use rho_harness_core::config::{Config, McpConfig, McpServerConfig};
use rig::tool::{ToolContext, ToolSet};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn temp_workspace() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mcp_integration_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn test_mcp_server_discovery_and_tool_invocation_end_to_end() {
    let workspace = temp_workspace();
    let server_script = workspace.join("mock_mcp_server.sh");

    let script_content = r#"#!/bin/sh
while IFS= read -r line; do
    if echo "$line" | grep -q '"method":"initialize"'; then
        id=$(echo "$line" | grep -o '"id":[0-9]*' | cut -d: -f2)
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{\"tools\":{}},\"serverInfo\":{\"name\":\"mock-fs\",\"version\":\"1.0\"}}}"
    elif echo "$line" | grep -q '"method":"tools/list"'; then
        id=$(echo "$line" | grep -o '"id":[0-9]*' | cut -d: -f2)
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"tools\":[{\"name\":\"fs_read\",\"description\":\"Read a file via MCP\",\"inputSchema\":{\"type\":\"object\",\"properties\":{\"path\":{\"type\":\"string\"}},\"required\":[\"path\"]}}]}}"
    elif echo "$line" | grep -q '"method":"tools/call"'; then
        id=$(echo "$line" | grep -o '"id":[0-9]*' | cut -d: -f2)
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"content from mock fs MCP\"}],\"isError\":false}}"
    fi
done
"#;

    std::fs::write(&server_script, script_content).unwrap();
    std::fs::set_permissions(&server_script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut mcp_servers = BTreeMap::new();
    mcp_servers.insert(
        "filesystem".to_string(),
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
        ..Config::default()
    };

    let tools = load_mcp_tools(&config, &workspace).await;
    let tool_names: Vec<String> = tools.iter().map(|d| d.name().to_string()).collect();

    assert!(
        tool_names.contains(&"filesystem_fs_read".to_string()),
        "expected 'filesystem_fs_read' in tool_names: {tool_names:?}"
    );

    let tool_set = ToolSet::from_dynamic_tools(tools);
    let mut ctx = ToolContext::new();
    let result = tool_set
        .execute("filesystem_fs_read", r#"{"path":"foo.txt"}"#, &mut ctx)
        .await;

    assert!(result.is_success());
    let output_text = result.output().as_text().unwrap_or_default();
    assert!(
        output_text.contains("content from mock fs MCP"),
        "expected result content in: {output_text}"
    );

    let _ = std::fs::remove_dir_all(&workspace);
}
