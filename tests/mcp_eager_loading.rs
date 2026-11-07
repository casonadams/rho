#![cfg(unix)]

use crate::{build_engine, mcp_config, temp_dir, write_script};
use rho_harness_core::config::{Config, McpServerConfig};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn with_dummy_provider_key() {
    unsafe {
        std::env::set_var("ANTHROPIC_API_KEY", "test-key-not-real");
    }
}

fn write_fast_mcp_server(workspace: &std::path::Path) -> PathBuf {
    let script_path = workspace.join("fast_mcp_server.sh");
    let script = "#!/bin/sh\nwhile IFS= read -r line; do\n  id=$(echo \"$line\" | grep -o '\"id\":[0-9]*' | cut -d: -f2)\n  case \"$line\" in\n    *\"initialize\"*) echo \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":$id,\\\"result\\\":{\\\"protocolVersion\\\":\\\"2024-11-05\\\",\\\"capabilities\\\":{\\\"tools\\\":{}},\\\"serverInfo\\\":{\\\"name\\\":\\\"fast-fs\\\",\\\"version\\\":\\\"1.0\\\"}}}\" ;;\n    *\"tools/list\"*) echo \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":$id,\\\"result\\\":{\\\"tools\\\":[{\\\"name\\\":\\\"fast_read\\\",\\\"inputSchema\\\":{}}]}}\" ;;\n  esac\ndone\n";
    write_script(&script_path, script);
    script_path
}

fn single_server_config(workspace: &Path, name: &str, cmd: &str) -> Config {
    let servers = BTreeMap::from([(name.to_string(), McpServerConfig::stdio(cmd, Vec::new()))]);
    mcp_config(workspace, servers)
}

async fn assert_eager_tools(workspace: &Path, name: &str, cmd: &str, expected: &[&str]) {
    let config = single_server_config(workspace, name, cmd);
    let tools = build_engine(workspace, config).await.tool_names();
    for tool in expected {
        assert!(tools.contains(&tool.to_string()), "missing tool {tool}");
    }
}

#[tokio::test]
async fn test_mcp_eager_loading_attaches_tools_before_first_turn() {
    with_dummy_provider_key();
    let workspace = temp_dir("mcp_lazy_test");
    let script = write_fast_mcp_server(&workspace);
    assert_eager_tools(
        &workspace,
        "fast",
        script.to_str().unwrap(),
        &["read", "fast_fast_read"],
    )
    .await;
    let _ = std::fs::remove_dir_all(&workspace);
}

#[tokio::test]
async fn test_mcp_eager_loading_resilient_to_server_failure() {
    let workspace = temp_dir("mcp_lazy_test");
    assert_eager_tools(&workspace, "broken", "/nonexistent/binary", &["read", "bash"]).await;
    let _ = std::fs::remove_dir_all(&workspace);
}
