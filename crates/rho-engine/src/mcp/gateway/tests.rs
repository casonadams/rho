use super::*;
use crate::mcp::client::{McpContent, McpToolResult};

#[test]
fn test_gateway_status_and_search() {
    let clients = BTreeMap::new();
    let def1 = McpToolDefinition {
        name: "snapshot".to_string(),
        description: Some("Take browser snapshot".to_string()),
        input_schema: json!({}),
    };
    let def2 = McpToolDefinition {
        name: "click".to_string(),
        description: Some("Click element".to_string()),
        input_schema: json!({}),
    };

    let tool_defs = vec![("playwright".to_string(), def1), ("playwright".to_string(), def2)];

    let gateway = McpGateway::new(clients, tool_defs, 1024);

    let search_results = gateway.search("snap");
    assert_eq!(search_results.len(), 1);
    assert_eq!(search_results[0]["name"], "snapshot");

    let desc = gateway.describe("playwright_click").unwrap();
    assert_eq!(desc["name"], "click");
    assert_eq!(desc["server"], "playwright");
}

#[test]
fn test_mcp_tool_result_truncation_and_images() {
    let res = McpToolResult {
        content: vec![
            McpContent {
                kind: "text".to_string(),
                text: Some("hello world this is a long text output".to_string()),
                data: None,
                mime_type: None,
            },
            McpContent {
                kind: "image".to_string(),
                text: None,
                data: Some("iVBORw0KGgoAAAANSUhEUg==".to_string()),
                mime_type: Some("image/png".to_string()),
            },
        ],
        is_error: Some(false),
    };

    let full_text = res.as_text();
    assert!(full_text.contains("hello world"));
    assert!(full_text.contains("[Image: image/png,"));

    let truncated = res.as_text_truncated(10);
    assert!(truncated.contains("output truncated"));
}

#[test]
fn test_resolve_target() {
    let clients = BTreeMap::new();
    let def = McpToolDefinition {
        name: "snapshot".to_string(),
        description: Some("Take browser snapshot".to_string()),
        input_schema: json!({}),
    };
    let gateway = McpGateway::new(clients, vec![("playwright".to_string(), def)], 1024);

    let call1 = McpSingleCall {
        server: Some("custom_server".to_string()),
        tool: "custom_tool".to_string(),
        args: json!({}),
    };
    assert_eq!(
        gateway.resolve_target(&call1).unwrap(),
        ("custom_server".to_string(), "custom_tool".to_string())
    );

    let call2 = McpSingleCall {
        server: None,
        tool: "snapshot".to_string(),
        args: json!({}),
    };
    assert_eq!(
        gateway.resolve_target(&call2).unwrap(),
        ("playwright".to_string(), "snapshot".to_string())
    );

    let call3 = McpSingleCall {
        server: None,
        tool: "filesystem_read_file".to_string(),
        args: json!({}),
    };
    assert_eq!(
        gateway.resolve_target(&call3).unwrap(),
        ("filesystem".to_string(), "read_file".to_string())
    );

    let call4 = McpSingleCall {
        server: None,
        tool: "unknown".to_string(),
        args: json!({}),
    };
    assert!(gateway.resolve_target(&call4).is_err());
}

async fn spawn_mock_mcp_server(
    handler: impl Fn(Value) -> Result<Value, String> + Send + Sync + 'static,
) -> (String, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}/mcp");
    let handler = Arc::new(handler);

    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            let handler = Arc::clone(&handler);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let Ok(n) = socket.read(&mut buf).await else {
                    return;
                };
                if n == 0 {
                    return;
                }
                let req_str = String::from_utf8_lossy(&buf[..n]);
                let Some((_headers, body)) = req_str.split_once("\r\n\r\n") else {
                    return;
                };
                let Ok(json_req) = serde_json::from_str::<Value>(body) else {
                    return;
                };
                let id = json_req.get("id").cloned().unwrap_or(Value::Null);

                let http_resp = match handler(json_req) {
                    Ok(result) => {
                        let resp_body = json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": result
                        })
                        .to_string();
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            resp_body.len(),
                            resp_body
                        )
                    }
                    Err(err_msg) => {
                        let resp_body = json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": { "code": -32603, "message": err_msg }
                        })
                        .to_string();
                        format!(
                            "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            resp_body.len(),
                            resp_body
                        )
                    }
                };
                let _ = socket.write_all(http_resp.as_bytes()).await;
            });
        }
    });

    (url, handle)
}

async fn create_mock_echo_gateway() -> McpGateway {
    let (url, _server) = spawn_mock_mcp_server(|req| {
        let tool_name = req["params"]["name"].as_str().unwrap_or("");
        match tool_name {
            "echo" => Ok(json!({
                "content": [{ "type": "text", "text": "echo result" }],
                "isError": false
            })),
            "fail" => Ok(json!({
                "content": [{ "type": "text", "text": "something failed" }],
                "isError": true
            })),
            "crash" => Err("server crashed".to_string()),
            _ => Ok(json!({
                "content": [{ "type": "text", "text": "ok" }],
            })),
        }
    })
    .await;

    let transport = crate::mcp::transport::McpTransport::new_http(
        url,
        rho_harness_core::config::McpTransportKind::StreamableHttp,
        BTreeMap::new(),
        None,
    );
    let client = Arc::new(McpClient::new("mock_srv", transport));

    let mut clients = BTreeMap::new();
    clients.insert("mock_srv".to_string(), client);

    let def = McpToolDefinition {
        name: "echo".to_string(),
        description: Some("Echo test".to_string()),
        input_schema: json!({ "type": "object" }),
    };
    McpGateway::new(clients, vec![("mock_srv".to_string(), def)], 1024)
}

#[tokio::test]
async fn test_gateway_call_and_status() {
    let gateway = create_mock_echo_gateway().await;

    let status = gateway.status();
    assert_eq!(status["total_tools"], 1);
    assert_eq!(status["connected_servers"]["mock_srv"]["tool_count"], 1);

    let res = gateway
        .call(McpSingleCall {
            server: Some("mock_srv".to_string()),
            tool: "echo".to_string(),
            args: json!({}),
        })
        .await
        .unwrap();
    assert_eq!(res, "echo result");

    let res_err = gateway
        .call(McpSingleCall {
            server: Some("mock_srv".to_string()),
            tool: "fail".to_string(),
            args: json!({}),
        })
        .await
        .unwrap();
    assert_eq!(res_err, "[Error] something failed");

    let err_unconnected = gateway
        .call(McpSingleCall {
            server: Some("unknown_srv".to_string()),
            tool: "echo".to_string(),
            args: json!({}),
        })
        .await
        .unwrap_err();
    assert!(err_unconnected.contains("not connected"));

    let err_crash = gateway
        .call(McpSingleCall {
            server: Some("mock_srv".to_string()),
            tool: "crash".to_string(),
            args: json!({}),
        })
        .await
        .unwrap_err();
    assert!(err_crash.contains("MCP Call failed"));
}

#[tokio::test]
async fn test_gateway_parsed_call_describe_and_search() {
    let gateway = create_mock_echo_gateway().await;

    let out_desc = execute_gateway_parsed_call(
        &gateway,
        McpGatewayArgs {
            describe: Some("mock_srv_echo".to_string()),
            ..Default::default()
        },
    )
    .await;
    assert!(out_desc.contains("\"name\": \"echo\""));

    let out_desc_missing = execute_gateway_parsed_call(
        &gateway,
        McpGatewayArgs {
            describe: Some("nonexistent".to_string()),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(out_desc_missing, "Tool 'nonexistent' not found");

    let out_search = execute_gateway_parsed_call(
        &gateway,
        McpGatewayArgs {
            search: Some("echo".to_string()),
            ..Default::default()
        },
    )
    .await;
    assert!(out_search.contains("mock_srv_echo"));
}

#[tokio::test]
async fn test_gateway_parsed_call_tool_and_status() {
    let gateway = create_mock_echo_gateway().await;

    let out_tool = execute_gateway_parsed_call(
        &gateway,
        McpGatewayArgs {
            server: Some("mock_srv".to_string()),
            tool: Some("echo".to_string()),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(out_tool, "echo result");

    let out_tool_err = execute_gateway_parsed_call(
        &gateway,
        McpGatewayArgs {
            server: Some("mock_srv".to_string()),
            tool: Some("crash".to_string()),
            ..Default::default()
        },
    )
    .await;
    assert!(out_tool_err.contains("[MCP Gateway Error]"));

    let out_default = execute_gateway_parsed_call(&gateway, McpGatewayArgs::default()).await;
    assert!(out_default.contains("connected_servers"));
}

#[tokio::test]
async fn test_gateway_batch_execution() {
    let gateway = create_mock_echo_gateway().await;

    let empty_batch = execute_mcp_batch(&gateway, vec![]).await;
    assert_eq!(empty_batch, "");

    let batch_ok = execute_mcp_batch(
        &gateway,
        vec![
            McpSingleCall {
                server: Some("mock_srv".to_string()),
                tool: "echo".to_string(),
                args: json!({}),
            },
            McpSingleCall {
                server: Some("mock_srv".to_string()),
                tool: "echo".to_string(),
                args: json!({}),
            },
        ],
    )
    .await;
    assert!(batch_ok.contains("[Call 1: echo]"));
    assert!(batch_ok.contains("[Call 2: echo]"));

    let batch_fail = execute_mcp_batch(
        &gateway,
        vec![
            McpSingleCall {
                server: Some("mock_srv".to_string()),
                tool: "echo".to_string(),
                args: json!({}),
            },
            McpSingleCall {
                server: Some("mock_srv".to_string()),
                tool: "crash".to_string(),
                args: json!({}),
            },
            McpSingleCall {
                server: Some("mock_srv".to_string()),
                tool: "echo".to_string(),
                args: json!({}),
            },
        ],
    )
    .await;
    assert!(batch_fail.contains("[Call 1: echo]"));
    assert!(batch_fail.contains("[Call 2: crash Failed]"));
    assert!(!batch_fail.contains("[Call 3: echo]"));
}

#[tokio::test]
async fn test_gateway_dynamic_tools() {
    let clients = BTreeMap::new();
    let gateway = McpGateway::new(clients, vec![], 1024);
    let (gw_tool, script_tool) = gateway.into_dynamic_tools();

    let tool_set = rig::tool::ToolSet::from_dynamic_tools(vec![gw_tool, script_tool]);
    let mut ctx = rig::tool::ToolContext::new();

    let res = tool_set.execute("mcp", r#"{"action":"status"}"#, &mut ctx).await;
    assert!(res.is_success());

    let res_script = tool_set.execute("mcpScript", r#"{"calls":[]}"#, &mut ctx).await;
    assert!(res_script.is_success());
    assert!(
        res_script
            .output()
            .as_text()
            .unwrap_or_default()
            .contains("No calls provided in batch")
    );

    let res_script_nonempty = tool_set
        .execute("mcpScript", r#"{"calls":[{"tool":"nonexistent"}]}"#, &mut ctx)
        .await;
    assert!(res_script_nonempty.is_success());
    assert!(
        res_script_nonempty
            .output()
            .as_text()
            .unwrap_or_default()
            .contains("Unknown tool: nonexistent")
    );
}
