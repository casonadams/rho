use super::*;
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, duplex};

struct TestGuardPlugin;

#[async_trait]
impl Plugin for TestGuardPlugin {
    fn name(&self) -> &str {
        "test_guard"
    }

    async fn on_event(&self, event: StepEvent, ctx: &HostContext) -> Flow {
        match event {
            StepEvent::ToolCall { tool_name, args } => {
                if tool_name == "bash" && args.get("command").and_then(Value::as_str) == Some("rm -rf /") {
                    return Flow::skip("Blocked root deletion");
                }
                if tool_name == "bash" && args.get("command").and_then(Value::as_str) == Some("sudo reboot") {
                    let ok = ctx.confirm("Reboot System", "Allow reboot?").await;
                    if !ok {
                        return Flow::skip("Reboot denied by user");
                    }
                }
                Flow::cont()
            }
            StepEvent::InvalidToolCall { tool_name, .. } => {
                if tool_name == "sh" {
                    return Flow::repair("bash");
                }
                Flow::cont()
            }
            StepEvent::ToolResult { .. } => {
                ctx.block("Tool Result", "Success", "success").await;
                ctx.set_status("quota", Some("5h: 90%")).await;
                Flow::cont()
            }
            _ => Flow::cont(),
        }
    }
}

#[tokio::test]
async fn sdk_plugin_roundtrip_flow() {
    let (client_read, server_write) = duplex(1024);
    let (server_read, mut client_write) = duplex(1024);

    tokio::spawn(async move {
        serve_stdio(TestGuardPlugin, server_read, server_write).await;
    });

    let mut client_lines = BufReader::new(client_read).lines();

    // 1. Initialize
    client_write
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n")
        .await
        .unwrap();
    let init_line = client_lines.next_line().await.unwrap().unwrap();
    let init_val: Value = serde_json::from_str(&init_line).unwrap();
    assert_eq!(init_val["id"], 1);
    assert_eq!(init_val["result"]["serverInfo"]["name"], "test_guard");

    // 2. Allowed tool call
    client_write
        .write_all(
            b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"hook/tool_call\",\"params\":{\"event\":\"tool_call\",\"tool_name\":\"bash\",\"args\":{\"command\":\"ls\"}}}\n",
        )
        .await
        .unwrap();
    let allow_line = client_lines.next_line().await.unwrap().unwrap();
    let allow_val: Value = serde_json::from_str(&allow_line).unwrap();
    assert_eq!(allow_val["id"], 2);
    assert_eq!(allow_val["result"]["action"], "continue");

    // 3. Denied tool call
    client_write
        .write_all(
            b"{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"hook/tool_call\",\"params\":{\"event\":\"tool_call\",\"tool_name\":\"bash\",\"args\":{\"command\":\"rm -rf /\"}}}\n",
        )
        .await
        .unwrap();
    let deny_line = client_lines.next_line().await.unwrap().unwrap();
    let deny_val: Value = serde_json::from_str(&deny_line).unwrap();
    assert_eq!(deny_val["id"], 3);
    assert_eq!(deny_val["result"]["action"], "skip");
    assert_eq!(deny_val["result"]["reason"], "Blocked root deletion");

    // 4. Interactive confirm
    client_write
        .write_all(
            b"{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"hook/tool_call\",\"params\":{\"event\":\"tool_call\",\"tool_name\":\"bash\",\"args\":{\"command\":\"sudo reboot\"}}}\n",
        )
        .await
        .unwrap();

    // Plugin sends host/ui/confirm
    let host_req_line = client_lines.next_line().await.unwrap().unwrap();
    let host_req: Value = serde_json::from_str(&host_req_line).unwrap();
    assert_eq!(host_req["method"], "host/ui/confirm");
    let host_req_id = host_req["id"].as_u64().unwrap();

    // Client replies with confirmed: false
    let reply = json!({"jsonrpc": "2.0", "id": host_req_id, "result": {"confirmed": false}});
    client_write.write_all(format!("{reply}\n").as_bytes()).await.unwrap();

    // Plugin receives confirmation and responds to tool call
    let confirm_res_line = client_lines.next_line().await.unwrap().unwrap();
    let confirm_res: Value = serde_json::from_str(&confirm_res_line).unwrap();
    assert_eq!(confirm_res["id"], 4);
    assert_eq!(confirm_res["result"]["action"], "skip");
    assert_eq!(confirm_res["result"]["reason"], "Reboot denied by user");

    // 5. Tool result -> sends host/ui/block and host/ui/set_status
    client_write
        .write_all(
            b"{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"hook/tool_result\",\"params\":{\"event\":\"tool_result\",\"tool_name\":\"bash\",\"args\":{},\"output\":\"ok\",\"is_error\":false}}\n",
        )
        .await
        .unwrap();

    let block_req_line = client_lines.next_line().await.unwrap().unwrap();
    let block_req: Value = serde_json::from_str(&block_req_line).unwrap();
    assert_eq!(block_req["method"], "host/ui/block");
    let block_id = block_req["id"].as_u64().unwrap();
    client_write
        .write_all(format!("{{\"jsonrpc\":\"2.0\",\"id\":{block_id},\"result\":{{\"success\":true}}}}\n").as_bytes())
        .await
        .unwrap();

    let status_req_line = client_lines.next_line().await.unwrap().unwrap();
    let status_req: Value = serde_json::from_str(&status_req_line).unwrap();
    assert_eq!(status_req["method"], "host/ui/set_status");
    let status_id = status_req["id"].as_u64().unwrap();
    client_write
        .write_all(format!("{{\"jsonrpc\":\"2.0\",\"id\":{status_id},\"result\":{{\"success\":true}}}}\n").as_bytes())
        .await
        .unwrap();

    let tool_res_line = client_lines.next_line().await.unwrap().unwrap();
    let tool_res: Value = serde_json::from_str(&tool_res_line).unwrap();
    assert_eq!(tool_res["id"], 5);
    assert_eq!(tool_res["result"]["action"], "continue");
}
