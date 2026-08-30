#![cfg(unix)]

use async_trait::async_trait;
use rho::approval::{ApprovalCapability, ApprovalDecision, ApprovalEventSink, ApprovalRequest, approval_context};
use rho::config::{Config, McpConfig, McpServerConfig};
use rho::engine::provider::host_loop::{NeutralToolCall, NeutralToolExecutor};
use rho::plugin::tool_dispatch::ActiveToolSet;
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;

struct TestApprovalSink;

#[async_trait]
impl ApprovalEventSink for TestApprovalSink {
    async fn request_approval(&self, _request: ApprovalRequest) -> ApprovalDecision {
        ApprovalDecision::Approved
    }
}

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
        auto_approve: true,
        ..Config::default()
    };

    let active = ActiveToolSet::load(&config, &workspace).await.unwrap();
    let tool_names: Vec<String> = active.definitions().iter().map(|d| d.id.name().to_string()).collect();

    assert!(
        tool_names.contains(&"fs_read".to_string()),
        "expected 'fs_read' in tool_names: {tool_names:?}"
    );

    let approval_cap = ApprovalCapability::new(true, Arc::new(TestApprovalSink));
    let executor = Arc::new(active).neutral_executor(approval_context(approval_cap));

    let result = executor
        .execute(NeutralToolCall {
            call_id: "call-1".to_string(),
            tool_id: "tool:fs_read".parse().unwrap(),
            arguments: serde_json::json!({"path": "dummy.txt"}),
        })
        .await
        .unwrap();

    assert_eq!(result.content, "content from mock fs MCP");
    assert!(!result.is_error);

    let _ = std::fs::remove_dir_all(workspace);
}
