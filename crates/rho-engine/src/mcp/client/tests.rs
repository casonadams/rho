use super::*;
use crate::mcp::process::McpProcess;
use rho_harness_core::config::McpServerConfig;

fn mock_mcp_script() -> &'static str {
    "read l; id=$(echo \"$l\" | grep -o '\"id\":[0-9]*' | cut -d: -f2)\necho \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":$id,\\\"result\\\":{\\\"protocolVersion\\\":\\\"2024-11-05\\\",\\\"capabilities\\\":{\\\"tools\\\":{}},\\\"serverInfo\\\":{\\\"name\\\":\\\"mock-server\\\"}}}\"\nread n\nread l; id=$(echo \"$l\" | grep -o '\"id\":[0-9]*' | cut -d: -f2)\necho \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":$id,\\\"result\\\":{\\\"tools\\\":[{\\\"name\\\":\\\"mock_tool\\\",\\\"inputSchema\\\":{}}]}}\"\n"
}

#[cfg(unix)]
fn spawn_mock_client(script: &str) -> McpClient {
    let config = McpServerConfig::stdio("/bin/sh", vec!["-c".to_string(), script.to_string()]);
    let dir = crate::test_support::TestDir::new();
    let (stdin, stdout, handle) = McpProcess::spawn(&config, &dir).unwrap();
    McpClient::new("mock", McpTransport::new_stdio(stdin, stdout, handle, None))
}

#[cfg(unix)]
#[tokio::test]
async fn test_mcp_client_handshake_and_tools_list() {
    let client = spawn_mock_client(mock_mcp_script());

    let init_resp = client.initialize().await.unwrap();
    assert!(init_resp.get("serverInfo").is_some());
    let tools = client.list_tools().await.unwrap();
    assert_eq!((tools.len(), tools[0].name.as_str()), (1, "mock_tool"));
}

#[tokio::test]
async fn initialize_fails_when_the_server_rejects_the_initialized_notification() {
    use crate::mcp::test_server::read_request;
    use tokio::io::AsyncWriteExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let body = r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{}}}"#;
        let responses = [
            format!(
                "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ),
            "HTTP/1.1 500 Internal Server Error\r\nConnection: close\r\nContent-Length: 0\r\n\r\n".to_string(),
        ];
        for response in responses {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_request(&mut socket).await;
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });
    let transport = McpTransport::new_http(
        url,
        rho_harness_core::config::McpTransportKind::StreamableHttp,
        BTreeMap::new(),
        None,
        None,
    );

    let err = McpClient::new("http", transport).initialize().await.unwrap_err();

    assert!(err.to_string().contains("HTTP error 500"), "{err}");
}

fn mock_mcp_extended_script() -> &'static str {
    "read l; id=$(echo \"$l\" | grep -o '\"id\":[0-9]*' | cut -d: -f2)\necho \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":$id,\\\"result\\\":{\\\"protocolVersion\\\":\\\"2025-11-25\\\",\\\"capabilities\\\":{\\\"resources\\\":{},\\\"prompts\\\":{}},\\\"serverInfo\\\":{\\\"name\\\":\\\"extended-mock\\\"}}}\"\nread n\nread l; id=$(echo \"$l\" | grep -o '\"id\":[0-9]*' | cut -d: -f2)\necho \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":$id,\\\"result\\\":{\\\"resources\\\":[{\\\"uri\\\":\\\"memo://notes\\\",\\\"name\\\":\\\"Notes\\\"}]}}\"\nread l; id=$(echo \"$l\" | grep -o '\"id\":[0-9]*' | cut -d: -f2)\necho \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":$id,\\\"result\\\":{\\\"prompts\\\":[{\\\"name\\\":\\\"review\\\",\\\"description\\\":\\\"Review PR\\\",\\\"arguments\\\":[]}]}}\"\n"
}

#[cfg(unix)]
#[tokio::test]
async fn test_mcp_client_resources_and_prompts() {
    let client = spawn_mock_client(mock_mcp_extended_script());

    let _ = client.initialize().await.unwrap();

    let resources = client.list_resources().await.unwrap();
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0].uri, "memo://notes");

    let prompts = client.list_prompts().await.unwrap();
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].name, "review");
}
