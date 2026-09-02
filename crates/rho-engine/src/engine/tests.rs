use super::*;
use crate::auth::AuthStore;
use crate::mcp::load_mcp_tools;
use rho_harness_core::config::{Config, McpConfig, McpServerConfig};
use rig::memory::ConversationMemory;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("engine_{label}_{}", uuid::Uuid::new_v4()))
}

fn test_config(label: &str) -> (Config, PathBuf) {
    let dir = temp_dir(label);
    std::fs::create_dir_all(&dir).unwrap();
    let mut config = Config::default();
    config.sessions_dir = dir.join("sessions");
    config.auth_file = dir.join("auth.json");
    (config, dir)
}

// Engine construction goes through ProviderFactory; a dummy key is fine because
// client construction never contacts the network.
fn with_dummy_provider_key() {
    unsafe {
        std::env::set_var("ANTHROPIC_API_KEY", "test-key-not-real");
    }
}

fn mock_mcp_server(workspace: &PathBuf) -> (String, String) {
    let script = workspace.join(format!("mock_mcp_server_{}.sh", uuid::Uuid::new_v4().simple()));
    std::fs::write(
        &script,
        r#"#!/bin/sh
while IFS= read -r line; do
    if echo "$line" | grep -q '"method":"initialize"'; then
        id=$(echo "$line" | grep -o '"id":[0-9]*' | cut -d: -f2)
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{\"tools\":{}},\"serverInfo\":{\"name\":\"mock\",\"version\":\"1.0\"}}}"
    elif echo "$line" | grep -q '"method":"tools/list"'; then
        id=$(echo "$line" | grep -o '"id":[0-9]*' | cut -d: -f2)
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"tools\":[{\"name\":\"ping\",\"description\":\"Mock ping\",\"inputSchema\":{\"type\":\"object\"}}]}}"
    fi
done
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let command = script.to_str().unwrap().to_string();
    let pattern = script.file_name().unwrap().to_string_lossy().to_string();
    (command, pattern)
}

fn with_mock_mcp(mut config: Config, command: String) -> Config {
    let mut servers = BTreeMap::new();
    servers.insert(
        "mock".to_string(),
        McpServerConfig {
            command,
            args: Vec::new(),
            env: BTreeMap::new(),
            enabled: true,
        },
    );
    config.mcp = McpConfig { enabled: true, servers };
    config
}

fn count_server_processes(pattern: &str) -> usize {
    let output = std::process::Command::new("pgrep")
        .args(["-f", pattern])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
}

async fn wait_for_server_count(pattern: &str, expected: usize) {
    for _ in 0..40 {
        if count_server_processes(pattern) == expected {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!(
        "expected {expected} mock MCP server process(es) matching '{pattern}', found {}",
        count_server_processes(pattern)
    );
}

#[tokio::test]
async fn rebuild_preserves_session_history_and_reattaches_tools() {
    with_dummy_provider_key();
    let (config, dir) = test_config("continuity");
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let base_dir = std::env::temp_dir();
    let tools = crate::tools::build_builtin_tools(&base_dir, &config).unwrap();
    assert!(!tools.is_empty(), "builtin tools should be assembled");

    let engine = builder::AgentEngineBuilder::new(config.clone(), auth_store.clone())
        .base_dir(base_dir.clone())
        .tools(tools.clone())
        .build()
        .await
        .unwrap();
    assert_eq!(engine.tool_names.len(), tools.len());

    let session_id = engine.session_manager.session_id.clone();
    engine
        .session_manager
        .append(
            &session_id,
            vec![
                rig::message::Message::user("remember this line"),
                rig::message::Message::assistant("recorded"),
            ],
        )
        .await
        .unwrap();
    let history_before = engine.session_manager.load(&session_id).await.unwrap();
    assert_eq!(history_before.len(), 2);
    let jsonl = config.sessions_dir.join(format!("{session_id}.jsonl"));
    let jsonl_before = std::fs::read(&jsonl).unwrap();

    let mut new_config = config.clone();
    new_config.max_turns = 7;
    let rebuilt = engine.rebuild(new_config, auth_store.clone()).await.unwrap();

    assert_eq!(rebuilt.config.max_turns, 7);
    assert_eq!(rebuilt.session_manager.session_id, session_id);
    assert_eq!(rebuilt.tool_names.len(), tools.len());
    let history_after = rebuilt.session_manager.load(&session_id).await.unwrap();
    assert_eq!(history_after, history_before);
    assert_eq!(std::fs::read(&jsonl).unwrap(), jsonl_before);

    std::fs::remove_dir_all(dir).unwrap();
}

/// Rebuild must re-resolve MCP tools (REQ-004) and dropping the old engine must
/// reap its MCP child (no process leak across reloads).
#[cfg(unix)]
#[tokio::test]
async fn rebuild_respawns_mcp_tools_and_reaps_previous_children() {
    with_dummy_provider_key();
    let (config, dir) = test_config("mcp_rebuild");
    let workspace = dir.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let (script_command, script_pattern) = mock_mcp_server(&workspace);
    let config = with_mock_mcp(config, script_command);

    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let tools = load_mcp_tools(&config, &workspace).await;
    assert!(
        tools.iter().any(|t| t.name() == "mock_ping"),
        "mock MCP server should expose mock_ping"
    );

    let engine = builder::AgentEngineBuilder::new(config.clone(), auth_store.clone())
        .base_dir(workspace.clone())
        .tools(tools)
        .build()
        .await
        .unwrap();
    wait_for_server_count(&script_pattern, 1).await;

    let rebuilt = engine.rebuild(config.clone(), auth_store.clone()).await.unwrap();
    assert!(
        rebuilt.tool_names.iter().any(|name| name == "mock_ping"),
        "rebuild must re-attach MCP tools, got: {:?}",
        rebuilt.tool_names
    );
    // Old engine still alive here (rebuild borrows); drop it and confirm its
    // child is reaped while the rebuilt engine's child keeps running.
    drop(engine);
    wait_for_server_count(&script_pattern, 1).await;

    std::fs::remove_dir_all(dir).unwrap();
}

/// Repeated reloads stay at one live MCP child per configured server.
#[cfg(unix)]
#[tokio::test]
async fn repeated_rebuilds_do_not_leak_mcp_children() {
    with_dummy_provider_key();
    let (config, dir) = test_config("mcp_no_leak");
    let workspace = dir.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let (script_command, script_pattern) = mock_mcp_server(&workspace);
    let config = with_mock_mcp(config, script_command);

    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let engine = builder::AgentEngineBuilder::new(config.clone(), auth_store.clone())
        .base_dir(workspace.clone())
        .build()
        .await
        .unwrap();

    let mut current = engine;
    for _ in 0..3 {
        current = current.rebuild(config.clone(), auth_store.clone()).await.unwrap();
        wait_for_server_count(&script_pattern, 1).await;
    }

    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn builder_attaches_dynamic_plugin_tools() {
    with_dummy_provider_key();
    let (config, dir) = test_config("plugin_tools");
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let base_dir = std::env::temp_dir();

    let custom_tool = rig::tool::DynamicTool::new(
        "generate_image",
        "Generate image tool",
        serde_json::json!({
            "type": "object",
            "properties": { "prompt": { "type": "string" } },
            "required": ["prompt"]
        }),
        |_ctx, _args| Box::pin(async { Ok(rig::tool::ToolOutput::text("image.png")) }),
    );

    let engine = builder::AgentEngineBuilder::new(config.clone(), auth_store.clone())
        .base_dir(base_dir)
        .add_tool(custom_tool)
        .build()
        .await
        .unwrap();

    assert!(engine.tool_names.contains(&"generate_image".to_string()));
    assert!(engine.tool_names.contains(&"read".to_string()));
    assert!(engine.tool_names.contains(&"bash".to_string()));

    std::fs::remove_dir_all(dir).unwrap();
}
