use super::*;
use crate::mcp::process::McpProcess;
use rho_harness_core::config::McpServerConfig;
use std::time::Duration;

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
    let transport = McpTransport::new_stdio(stdin, stdout, handle, None);

    let result = transport.request("ping", None).await.unwrap();

    assert_eq!(result, serde_json::json!({"status": "ok"}));
}

#[tokio::test]
async fn stdio_server_requests_are_answered() {
    let script = r#"
read line
echo '{"jsonrpc":"2.0","id":"srv-1","method":"ping"}'
read ping_reply
echo '{"jsonrpc":"2.0","id":7,"method":"roots/list"}'
read roots_reply
echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":[$ping_reply,$roots_reply]}"
"#;
    let config = McpServerConfig::stdio("/bin/sh", vec!["-c".to_string(), script.to_string()]);
    let (stdin, stdout, handle) = McpProcess::spawn(&config, &std::env::temp_dir()).unwrap();
    let transport = McpTransport::new_stdio(stdin, stdout, handle, Some(Duration::from_secs(1)));

    let replies = transport.request("tools/list", None).await.unwrap();

    assert_eq!(
        replies,
        serde_json::json!([
            { "jsonrpc": "2.0", "id": "srv-1", "result": {} },
            { "jsonrpc": "2.0", "id": 7, "error": { "code": -32601, "message": "Method not found: roots/list" } }
        ])
    );
}

async fn progress_every(inbound: Inbound, period: Duration) {
    loop {
        tokio::time::sleep(period).await;
        inbound.dispatch(PROGRESS_NOTIFICATION);
    }
}

#[tokio::test(start_paused = true)]
async fn requests_time_out_after_an_idle_window_without_progress() {
    let inbound = Inbound::default();
    let slow = tokio::time::sleep(Duration::from_secs(100));
    assert!(
        wait_with_progress(slow, Duration::from_secs(30), &inbound)
            .await
            .is_none()
    );
}

#[tokio::test(start_paused = true)]
async fn progress_notifications_extend_the_deadline() {
    let inbound = Inbound::default();
    let ticker = tokio::spawn(progress_every(inbound.clone(), Duration::from_secs(20)));
    let slow = async {
        tokio::time::sleep(Duration::from_secs(100)).await;
        "done"
    };
    assert_eq!(
        wait_with_progress(slow, Duration::from_secs(30), &inbound).await,
        Some("done")
    );
    ticker.abort();
}

#[tokio::test(start_paused = true)]
async fn progress_cannot_extend_a_request_past_the_hard_cap() {
    let inbound = Inbound::default();
    let ticker = tokio::spawn(progress_every(inbound.clone(), Duration::from_secs(10)));
    let started = tokio::time::Instant::now();
    let never = std::future::pending::<()>();
    assert!(
        wait_with_progress(never, Duration::from_secs(30), &inbound)
            .await
            .is_none()
    );
    assert!(started.elapsed() >= MAX_PROGRESS_EXTENSION);
    ticker.abort();
}

#[test]
fn tool_calls_get_a_longer_default_timeout_and_a_progress_token() {
    assert_eq!(request_timeout("tools/call", None), DEFAULT_TOOL_CALL_TIMEOUT);
    assert_eq!(request_timeout("tools/list", None), DEFAULT_REQUEST_TIMEOUT);
    assert_eq!(
        request_timeout("tools/call", Some(Duration::from_secs(5))),
        Duration::from_secs(5)
    );

    let params = with_progress_token("tools/call", Some(serde_json::json!({ "name": "t" })), 9);
    assert_eq!(params.unwrap()["_meta"]["progressToken"], 9);
    assert_eq!(with_progress_token("tools/list", None, 9), None);
}

#[tokio::test]
async fn abandoned_requests_send_a_cancellation_notice() {
    use crate::mcp::test_server::{self, Reply, handler};
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = Arc::clone(&calls);
    let server = test_server::spawn(handler(move |method, params| {
        seen.lock().unwrap().push((method.clone(), params));
        async move {
            if method == "tools/call" {
                std::future::pending::<()>().await;
            }
            Reply::Result(serde_json::json!({}))
        }
    }))
    .await;
    let transport = McpTransport::new_http(
        server.url.clone(),
        McpTransportKind::StreamableHttp,
        BTreeMap::new(),
        None,
        None,
    );
    let request = {
        let transport = Arc::clone(&transport);
        tokio::spawn(async move {
            transport
                .request("tools/call", Some(serde_json::json!({ "name": "t" })))
                .await
        })
    };
    let has = |method: &str| calls.lock().unwrap().iter().any(|(m, _)| m == method);
    crate::mcp::test_server::eventually("tools/call to arrive", || has("tools/call")).await;
    request.abort();
    crate::mcp::test_server::eventually("cancellation", || has("notifications/cancelled")).await;

    let calls = calls.lock().unwrap();
    let call_id = calls.iter().find(|(m, _)| m == "tools/call").unwrap().1["_meta"]["progressToken"].clone();
    let cancelled = &calls.iter().find(|(m, _)| m == "notifications/cancelled").unwrap().1;
    assert_eq!(cancelled["requestId"], call_id);
}

#[tokio::test]
async fn stdio_errors_distinguish_server_errors_from_lost_connections() {
    use crate::mcp::transport::stdio::await_mcp_response;
    use crate::mcp::types::JsonRpcError;
    use tokio::sync::oneshot;

    let (tx, rx) = oneshot::channel();
    tx.send(Err(JsonRpcError {
        code: -32601,
        message: "Method not found".to_string(),
        data: None,
    }))
    .unwrap();
    let server_error = await_mcp_response(rx, "test_method").await.unwrap_err();
    assert!(matches!(server_error, AppError::Mcp(_)), "{server_error}");

    let (tx, rx) = oneshot::channel();
    drop(tx);
    let lost = await_mcp_response(rx, "test_method").await.unwrap_err();
    assert!(matches!(lost, AppError::McpConnectionLost(_)), "{lost}");
}

#[tokio::test]
async fn a_refused_connection_is_reported_as_not_delivered() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    drop(listener);
    let transport = McpTransport::new_http(
        url,
        rho_harness_core::config::McpTransportKind::StreamableHttp,
        Default::default(),
        None,
        None,
    );

    let err = transport.request("tools/call", None).await.unwrap_err();

    assert!(matches!(err, AppError::McpNotDelivered(_)), "{err}");
}
