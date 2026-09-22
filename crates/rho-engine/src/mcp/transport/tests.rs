use super::*;
use crate::mcp::process::McpProcess;
use rho_harness_core::config::McpServerConfig;

#[tokio::test]
async fn test_mcp_transport_request_response() {
    // A simple shell script that reads one line and echoes back a JSON-RPC response
    let script = r#"
read line
id=$(echo "$line" | grep -o '"id":[0-9]*' | cut -d: -f2)
echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"status\":\"ok\"}}"
"#;
    let config = McpServerConfig::stdio("/bin/sh", vec!["-c".to_string(), script.to_string()]);

    let (stdin, stdout, handle) = McpProcess::spawn(&config, &std::env::temp_dir()).unwrap();
    let transport = McpTransport::new(stdin, stdout, handle);

    let result = transport.request("ping", None).await.unwrap();

    assert_eq!(result, serde_json::json!({"status": "ok"}));
}

#[tokio::test]
async fn test_await_mcp_response_error_paths() {
    use crate::mcp::transport::stdio::await_mcp_response;
    use crate::mcp::types::JsonRpcError;
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use std::time::Duration;
    use tokio::sync::oneshot;

    let pending = Mutex::new(BTreeMap::new());

    let (tx1, rx1) = oneshot::channel();
    tx1.send(Err(JsonRpcError {
        code: -32601,
        message: "Method not found".to_string(),
        data: None,
    }))
    .unwrap();
    let err1 = await_mcp_response(rx1, (&pending, 1), "test_method", Duration::from_secs(1))
        .await
        .unwrap_err();
    assert!(err1.to_string().contains("MCP error from test_method"));

    let (tx2, rx2) = oneshot::channel();
    drop(tx2);
    let err2 = await_mcp_response(rx2, (&pending, 2), "test_method", Duration::from_secs(1))
        .await
        .unwrap_err();
    assert!(err2.to_string().contains("closed stream"));

    let (_tx3, rx3) = oneshot::channel();
    let err3 = await_mcp_response(rx3, (&pending, 3), "test_method", Duration::from_millis(10))
        .await
        .unwrap_err();
    assert!(err3.to_string().contains("timed out"));
}
