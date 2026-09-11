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

fn write_mock_mcp_server(workspace: &std::path::Path) -> PathBuf {
    let server_script = workspace.join("mock_mcp_server.sh");
    let script = "#!/bin/sh\nwhile IFS= read -r line; do\n  id=$(echo \"$line\" | grep -o '\"id\":[0-9]*' | cut -d: -f2)\n  case \"$line\" in\n    *\"initialize\"*) echo \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":$id,\\\"result\\\":{\\\"protocolVersion\\\":\\\"2024-11-05\\\",\\\"capabilities\\\":{\\\"tools\\\":{}},\\\"serverInfo\\\":{\\\"name\\\":\\\"mock-fs\\\",\\\"version\\\":\\\"1.0\\\"}}}\" ;;\n    *\"tools/list\"*) echo \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":$id,\\\"result\\\":{\\\"tools\\\":[{\\\"name\\\":\\\"fs_read\\\",\\\"description\\\":\\\"Read via MCP\\\",\\\"inputSchema\\\":{\\\"type\\\":\\\"object\\\"}}]}}\" ;;\n    *\"tools/call\"*) echo \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":$id,\\\"result\\\":{\\\"content\\\":[{\\\"type\\\":\\\"text\\\",\\\"text\\\":\\\"content from mock fs MCP\\\"}],\\\"isError\\\":false}}\" ;;\n  esac\ndone\n";
    std::fs::write(&server_script, script).unwrap();
    std::fs::set_permissions(&server_script, std::fs::Permissions::from_mode(0o755)).unwrap();
    server_script
}

fn mcp_fs_config(script: &std::path::Path) -> Config {
    let mut servers = BTreeMap::new();
    servers.insert(
        "filesystem".to_string(),
        McpServerConfig::stdio(script.to_str().unwrap(), Vec::new()),
    );
    Config {
        mcp: McpConfig { enabled: true, servers },
        ..Config::default()
    }
}

#[tokio::test]
async fn test_mcp_server_discovery_and_tool_invocation_end_to_end() {
    let workspace = temp_workspace();
    let script = write_mock_mcp_server(&workspace);
    let config = mcp_fs_config(&script);

    let tools = load_mcp_tools(&config, &workspace).await;
    let tool_names: Vec<String> = tools.iter().map(|d| d.name().to_string()).collect();
    assert!(tool_names.contains(&"filesystem_fs_read".to_string()));

    let tool_set = ToolSet::from_dynamic_tools(tools);
    let result = tool_set
        .execute("filesystem_fs_read", r#"{"path":"foo.txt"}"#, &mut ToolContext::new())
        .await;
    assert!(result.is_success());
    let text = result.output().as_text().unwrap_or_default();
    assert!(text.contains("content from mock fs MCP"));
    let _ = std::fs::remove_dir_all(&workspace);
}
