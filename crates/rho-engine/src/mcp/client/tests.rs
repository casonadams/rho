use super::*;
use crate::mcp::process::McpProcess;
use rho_harness_core::config::McpServerConfig;
use std::collections::BTreeMap;

#[test]
fn test_mcp_tool_result_as_text() {
    let result = McpToolResult {
        content: vec![
            McpContent {
                kind: "text".to_string(),
                text: Some("line 1".to_string()),
                data: None,
                mime_type: None,
            },
            McpContent {
                kind: "text".to_string(),
                text: Some("line 2".to_string()),
                data: None,
                mime_type: None,
            },
        ],
        is_error: Some(false),
    };
    assert_eq!(result.as_text(), "line 1\nline 2");
}

#[test]
fn test_mcp_tool_result_as_text_bounded_multibyte_boundary() {
    let result = McpToolResult {
        content: vec![McpContent {
            kind: "text".to_string(),
            text: Some("hello 🦀 world".to_string()),
            data: None,
            mime_type: None,
        }],
        is_error: Some(false),
    };
    // "hello " is 6 bytes, "🦀" is 4 bytes (indices 6..10).
    // Cutting at 7, 8, or 9 bytes falls in the middle of '🦀'.
    let bounded = result.as_text_truncated(8);
    assert!(bounded.starts_with("hello \n[MCP tool output truncated at 8 bytes]"));
}

#[cfg(unix)]
#[tokio::test]
async fn test_mcp_client_handshake_and_tools_list() {
    let script = r#"
# Read initialize request
read init_line
init_id=$(echo "$init_line" | grep -o '"id":[0-9]*' | cut -d: -f2)
echo "{\"jsonrpc\":\"2.0\",\"id\":$init_id,\"result\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{\"tools\":{}},\"serverInfo\":{\"name\":\"mock-server\",\"version\":\"1.0\"}}}"

# Read notifications/initialized
read notif_line

# Read tools/list request
read list_line
list_id=$(echo "$list_line" | grep -o '"id":[0-9]*' | cut -d: -f2)
echo "{\"jsonrpc\":\"2.0\",\"id\":$list_id,\"result\":{\"tools\":[{\"name\":\"mock_tool\",\"description\":\"a mock tool\",\"inputSchema\":{\"type\":\"object\"}}]}}"
"#;
    let config = McpServerConfig {
        command: "/bin/sh".to_string(),
        args: vec!["-c".to_string(), script.to_string()],
        env: BTreeMap::new(),
        enabled: true,
    };

    let (stdin, stdout, handle) = McpProcess::spawn(&config, &std::env::temp_dir()).unwrap();
    let transport = McpTransport::new(stdin, stdout, handle);
    let client = McpClient::new("mock", transport);

    let init_resp = client.initialize().await.unwrap();
    assert!(init_resp.get("serverInfo").is_some());

    let tools = client.list_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "mock_tool");
}
