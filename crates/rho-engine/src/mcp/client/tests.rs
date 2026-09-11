use super::*;
use crate::mcp::process::McpProcess;
use rho_harness_core::config::McpServerConfig;

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

fn mock_mcp_script() -> &'static str {
    "read l; id=$(echo \"$l\" | grep -o '\"id\":[0-9]*' | cut -d: -f2)\necho \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":$id,\\\"result\\\":{\\\"protocolVersion\\\":\\\"2024-11-05\\\",\\\"capabilities\\\":{\\\"tools\\\":{}},\\\"serverInfo\\\":{\\\"name\\\":\\\"mock-server\\\"}}}\"\nread n\nread l; id=$(echo \"$l\" | grep -o '\"id\":[0-9]*' | cut -d: -f2)\necho \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":$id,\\\"result\\\":{\\\"tools\\\":[{\\\"name\\\":\\\"mock_tool\\\",\\\"inputSchema\\\":{}}]}}\"\n"
}

#[cfg(unix)]
#[tokio::test]
async fn test_mcp_client_handshake_and_tools_list() {
    let config = McpServerConfig::stdio("/bin/sh", vec!["-c".to_string(), mock_mcp_script().to_string()]);
    let (stdin, stdout, handle) = McpProcess::spawn(&config, &std::env::temp_dir()).unwrap();
    let client = McpClient::new("mock", McpTransport::new(stdin, stdout, handle));

    let init_resp = client.initialize().await.unwrap();
    assert!(init_resp.get("serverInfo").is_some());
    let tools = client.list_tools().await.unwrap();
    assert_eq!((tools.len(), tools[0].name.as_str()), (1, "mock_tool"));
}

fn mock_mcp_extended_script() -> &'static str {
    "read l; id=$(echo \"$l\" | grep -o '\"id\":[0-9]*' | cut -d: -f2)\necho \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":$id,\\\"result\\\":{\\\"protocolVersion\\\":\\\"2025-11-25\\\",\\\"capabilities\\\":{\\\"resources\\\":{},\\\"prompts\\\":{}},\\\"serverInfo\\\":{\\\"name\\\":\\\"extended-mock\\\"}}}\"\nread n\nread l; id=$(echo \"$l\" | grep -o '\"id\":[0-9]*' | cut -d: -f2)\necho \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":$id,\\\"result\\\":{\\\"resources\\\":[{\\\"uri\\\":\\\"memo://notes\\\",\\\"name\\\":\\\"Notes\\\"}]}}\"\nread l; id=$(echo \"$l\" | grep -o '\"id\":[0-9]*' | cut -d: -f2)\necho \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":$id,\\\"result\\\":{\\\"contents\\\":[{\\\"uri\\\":\\\"memo://notes\\\",\\\"text\\\":\\\"sample memo content\\\"}]}}\"\nread l; id=$(echo \"$l\" | grep -o '\"id\":[0-9]*' | cut -d: -f2)\necho \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":$id,\\\"result\\\":{\\\"prompts\\\":[{\\\"name\\\":\\\"review\\\",\\\"description\\\":\\\"Review PR\\\",\\\"arguments\\\":[]}]}}\"\n"
}

#[cfg(unix)]
#[tokio::test]
async fn test_mcp_client_resources_and_prompts() {
    let config = McpServerConfig::stdio(
        "/bin/sh",
        vec!["-c".to_string(), mock_mcp_extended_script().to_string()],
    );
    let (stdin, stdout, handle) = McpProcess::spawn(&config, &std::env::temp_dir()).unwrap();
    let client = McpClient::new("mock", McpTransport::new(stdin, stdout, handle));

    let _ = client.initialize().await.unwrap();

    let resources = client.list_resources().await.unwrap();
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0].uri, "memo://notes");

    let contents = client.read_resource("memo://notes").await.unwrap();
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0].text.as_deref(), Some("sample memo content"));

    let prompts = client.list_prompts().await.unwrap();
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].name, "review");
}
