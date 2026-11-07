use crate::{build_engine, mcp_config, temp_dir, write_script};
use rho_engine::mcp::{McpCache, McpToolDefinition, compute_definition_hash};
use rho_harness_core::config::{McpLifecycleMode, McpServerConfig};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn write_mock_server_script(workspace: &Path) -> PathBuf {
    let script_name = format!("mock_lazy_mcp_{}.sh", uuid::Uuid::new_v4());
    let script_path = workspace.join(script_name);
    let script = r#"#!/bin/sh
while IFS= read -r line; do
  id=$(echo "$line" | grep -o '"id":[0-9]*' | cut -d: -f2)
  case "$line" in
    *"initialize"*)
      echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"serverInfo\":{\"name\":\"mock\",\"version\":\"1.0\"}}}"
      ;;
    *"tools/list"*)
      echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"tools\":[{\"name\":\"ping\",\"description\":\"ping pong tool\",\"inputSchema\":{}}]}}"
      ;;
    *"tools/call"*)
      echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"pong\"}]}}"
      ;;
  esac
done
"#;
    write_script(&script_path, script);
    script_path
}

fn count_processes(pattern: &str) -> usize {
    let output = std::process::Command::new("pgrep")
        .args(["-f", pattern])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
}

#[tokio::test]
async fn test_cached_servers_launch_with_zero_child_processes() {
    let workspace = temp_dir("mcp_lazy_lifecycle_test");
    let script_path = write_mock_server_script(&workspace);
    let pattern = script_path.file_name().unwrap().to_str().unwrap();

    let mut cfg = McpServerConfig::stdio(script_path.to_str().unwrap(), Vec::new());
    cfg.lifecycle = Some(McpLifecycleMode::Lazy);
    let def_hash = compute_definition_hash(&cfg);

    let mut cache = McpCache::default();
    cache.update(
        "mock_srv",
        def_hash,
        vec![McpToolDefinition {
            name: "ping".to_string(),
            description: Some("ping pong tool".to_string()),
            input_schema: serde_json::json!({}),
        }],
        None,
    );
    cache.save_to_dir(&workspace).unwrap();

    let mut servers = BTreeMap::new();
    servers.insert("mock_srv".to_string(), cfg);

    let config = mcp_config(&workspace, servers);

    let engine = build_engine(&workspace, config).await;

    assert!(engine.tool_names().contains(&"mock_srv_ping".to_string()));
    assert_eq!(count_processes(pattern), 0);

    let ping_tool = {
        let tools = engine.tools();
        let tools_guard = tools.read().await;
        tools_guard
            .iter()
            .find(|t| t.name() == "mock_srv_ping")
            .cloned()
            .unwrap()
    };
    let res = ping_tool.execute(serde_json::json!({})).await.unwrap();
    assert_eq!(res.content, "pong");
    assert_eq!(count_processes(pattern), 1);

    let _ = std::fs::remove_dir_all(&workspace);
}
