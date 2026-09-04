use super::*;
use crate::mcp::process::McpProcess;
use rho_harness_core::config::McpServerConfig;
use std::collections::BTreeMap;

#[tokio::test]
async fn test_mcp_transport_request_response() {
    // A simple shell script that reads one line and echoes back a JSON-RPC response
    let script = r#"
read line
id=$(echo "$line" | grep -o '"id":[0-9]*' | cut -d: -f2)
echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"status\":\"ok\"}}"
"#;
    let config = McpServerConfig {
        command: "/bin/sh".to_string(),
        args: vec!["-c".to_string(), script.to_string()],
        env: BTreeMap::new(),
        enabled: true,
    };

    let (stdin, stdout, handle) = McpProcess::spawn(&config, &std::env::temp_dir()).unwrap();
    let transport = McpTransport::new(stdin, stdout, handle);

    let result = transport.request("ping", None).await.unwrap();

    assert_eq!(result, serde_json::json!({"status": "ok"}));
}
