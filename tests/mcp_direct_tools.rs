use rho_engine::auth::AuthStore;
use rho_engine::engine::builder::AgentEngineBuilder;
use rho_engine::mcp::{McpCache, McpToolDefinition, compute_definition_hash};
use rho_harness_core::config::{Config, McpConfig, McpDirectTools, McpLifecycleMode, McpServerConfig};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn temp_workspace() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mcp_dt_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_mock_tools_script(workspace: &Path) -> PathBuf {
    let script_path = workspace.join("mock_tools.sh");
    let script = r#"#!/bin/sh
while IFS= read -r line; do
  id=$(echo "$line" | grep -o '"id":[0-9]*' | cut -d: -f2)
  case "$line" in
    *"initialize"*)
      echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"serverInfo\":{\"name\":\"mock\",\"version\":\"1.0\"}}}"
      ;;
    *"tools/list"*)
      echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"tools\":[{\"name\":\"direct_tool\",\"description\":\"direct tool description\",\"inputSchema\":{}},{\"name\":\"deferred_tool\",\"description\":\"deferred tool description\",\"inputSchema\":{}}]}}"
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

#[tokio::test]
async fn test_direct_tools_list_filters_and_search_keywords() {
    let workspace = temp_workspace();
    let script_path = write_mock_tools_script(&workspace);

    let mut cfg = McpServerConfig::stdio(script_path.to_str().unwrap(), Vec::new());
    cfg.lifecycle = Some(McpLifecycleMode::Lazy);
    cfg.direct_tools = Some(McpDirectTools::List(vec!["direct_tool".to_string()]));
    let mut keywords = BTreeMap::new();
    keywords.insert("deferred_tool".to_string(), vec!["magic_alias".to_string()]);
    cfg.search_keywords = keywords;

    let def_hash = compute_definition_hash(&cfg);
    let mut cache = McpCache::default();
    cache.update(
        "srv",
        def_hash,
        vec![
            McpToolDefinition {
                name: "direct_tool".to_string(),
                description: Some("direct tool description".to_string()),
                input_schema: serde_json::json!({}),
            },
            McpToolDefinition {
                name: "deferred_tool".to_string(),
                description: Some("deferred tool description".to_string()),
                input_schema: serde_json::json!({}),
            },
        ],
        None,
    );
    cache.save_to_dir(&workspace).unwrap();

    let mut servers = BTreeMap::new();
    servers.insert("srv".to_string(), cfg);

    let config = Config {
        config_dir: workspace.clone(),
        sessions_dir: workspace.join("sessions"),
        auth_file: workspace.join("auth.json"),
        mcp: McpConfig {
            enabled: true,
            defer_threshold: 0,
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

    let tool_names = engine.tool_names();
    assert!(tool_names.contains(&"srv_direct_tool".to_string()));
    assert!(!tool_names.contains(&"srv_deferred_tool".to_string()));
    assert!(tool_names.contains(&"tool_search".to_string()));

    let mut context = rig::tool::ToolContext::default();
    let res = engine
        .tool_server_handle()
        .execute("tool_search", r#"{"query": "magic_alias"}"#, &mut context)
        .await;
    let out = res.output().as_text().unwrap();
    assert!(out.contains("srv_deferred_tool"));

    let _ = std::fs::remove_dir_all(&workspace);
}
