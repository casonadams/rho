use rho_engine::auth::AuthStore;
use rho_engine::engine::builder::AgentEngineBuilder;
use rho_engine::mcp::{McpCache, McpToolDefinition, compute_definition_hash};
use rho_harness_core::config::{Config, McpConfig, McpLifecycleMode, McpServerConfig};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn temp_workspace() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mcp_lazy_lifecycle_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

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
    std::fs::write(&script_path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
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
    let workspace = temp_workspace();
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

    let config = Config {
        config_dir: workspace.clone(),
        sessions_dir: workspace.join("sessions"),
        auth_file: workspace.join("auth.json"),
        mcp: McpConfig {
            enabled: true,
            defer_threshold: 10,
            idle_timeout_seconds: 600,
            servers,
        },
        ..Default::default()
    };

    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let engine = AgentEngineBuilder::new(config, auth_store)
        .base_dir(workspace.clone())
        .build()
        .await
        .unwrap();

    assert!(engine.tool_names().contains(&"mock_srv_ping".to_string()));
    assert_eq!(count_processes(pattern), 0);

    let mut context = rig::tool::ToolContext::default();
    let res = engine
        .tool_server_handle()
        .execute("mock_srv_ping", "{}", &mut context)
        .await;
    assert_eq!(res.output().as_text().unwrap(), "pong");
    assert_eq!(count_processes(pattern), 1);

    let _ = std::fs::remove_dir_all(&workspace);
}
