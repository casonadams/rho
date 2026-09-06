use super::*;
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, duplex};

struct TestGuardPlugin;

async fn handle_tool_call(tool_name: &str, args: &Value, ctx: &HostContext) -> Flow {
    let cmd = args.get("command").and_then(Value::as_str);
    if tool_name == "bash" && cmd == Some("rm -rf /") {
        return Flow::skip("Blocked root deletion");
    }
    if tool_name == "bash" && cmd == Some("sudo reboot") {
        let ok = ctx.confirm("Reboot System", "Allow reboot?").await;
        if !ok {
            return Flow::skip("Reboot denied by user");
        }
    }
    Flow::cont()
}

#[async_trait]
impl Plugin for TestGuardPlugin {
    fn name(&self) -> &str {
        "test_guard"
    }

    async fn on_event(&self, event: StepEvent, ctx: &HostContext) -> Flow {
        match event {
            StepEvent::ToolCall { tool_name, args } => handle_tool_call(&tool_name, &args, ctx).await,
            StepEvent::InvalidToolCall { tool_name, .. } if tool_name == "sh" => Flow::repair("bash"),
            StepEvent::ToolResult { .. } => {
                ctx.block(("Tool Result", "Success", "success")).await;
                ctx.set_status("quota", Some("5h: 90%")).await;
                Flow::cont()
            }
            _ => Flow::cont(),
        }
    }
}

type ClientReader = tokio::io::Lines<BufReader<tokio::io::DuplexStream>>;
type ClientWriter = tokio::io::DuplexStream;

async fn step_init(reader: &mut ClientReader, writer: &mut ClientWriter) {
    writer
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n")
        .await
        .unwrap();
    let line = reader.next_line().await.unwrap().unwrap();
    let val: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(val["id"], 1);
    assert_eq!(val["result"]["serverInfo"]["name"], "test_guard");
}

async fn step_allow_and_deny(reader: &mut ClientReader, writer: &mut ClientWriter) {
    writer
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"hook/tool_call\",\"params\":{\"event\":\"tool_call\",\"tool_name\":\"bash\",\"args\":{\"command\":\"ls\"}}}\n")
        .await
        .unwrap();
    let val: Value = serde_json::from_str(&reader.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(val["id"], 2);
    assert_eq!(val["result"]["action"], "continue");

    writer
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"hook/tool_call\",\"params\":{\"event\":\"tool_call\",\"tool_name\":\"bash\",\"args\":{\"command\":\"rm -rf /\"}}}\n")
        .await
        .unwrap();
    let deny: Value = serde_json::from_str(&reader.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(deny["id"], 3);
    assert_eq!(deny["result"]["action"], "skip");
    assert_eq!(deny["result"]["reason"], "Blocked root deletion");
}

async fn step_confirm(reader: &mut ClientReader, writer: &mut ClientWriter) {
    writer
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"hook/tool_call\",\"params\":{\"event\":\"tool_call\",\"tool_name\":\"bash\",\"args\":{\"command\":\"sudo reboot\"}}}\n")
        .await
        .unwrap();
    let req: Value = serde_json::from_str(&reader.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(req["method"], "host/ui/confirm");
    let req_id = req["id"].as_u64().unwrap();

    let reply = json!({"jsonrpc": "2.0", "id": req_id, "result": {"confirmed": false}});
    writer.write_all(format!("{reply}\n").as_bytes()).await.unwrap();

    let res: Value = serde_json::from_str(&reader.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(res["id"], 4);
    assert_eq!(res["result"]["action"], "skip");
    assert_eq!(res["result"]["reason"], "Reboot denied by user");
}

async fn step_tool_result(reader: &mut ClientReader, writer: &mut ClientWriter) {
    writer
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"hook/tool_result\",\"params\":{\"event\":\"tool_result\",\"tool_name\":\"bash\",\"args\":{},\"output\":\"ok\",\"is_error\":false}}\n")
        .await
        .unwrap();
    let block: Value = serde_json::from_str(&reader.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(block["method"], "host/ui/block");
    let b_id = block["id"].as_u64().unwrap();
    let resp = format!("{{\"jsonrpc\":\"2.0\",\"id\":{b_id},\"result\":{{\"success\":true}}}}\n");
    writer.write_all(resp.as_bytes()).await.unwrap();

    let status: Value = serde_json::from_str(&reader.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(status["method"], "host/ui/set_status");
    let s_id = status["id"].as_u64().unwrap();
    let s_resp = format!("{{\"jsonrpc\":\"2.0\",\"id\":{s_id},\"result\":{{\"success\":true}}}}\n");
    writer.write_all(s_resp.as_bytes()).await.unwrap();

    let res: Value = serde_json::from_str(&reader.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(res["id"], 5);
    assert_eq!(res["result"]["action"], "continue");
}

#[tokio::test]
async fn sdk_plugin_roundtrip_flow() {
    let (client_read, server_write) = duplex(1024);
    let (server_read, mut client_write) = duplex(1024);

    tokio::spawn(async move {
        serve_stdio(TestGuardPlugin, server_read, server_write).await;
    });

    let mut reader = BufReader::new(client_read).lines();
    step_init(&mut reader, &mut client_write).await;
    step_allow_and_deny(&mut reader, &mut client_write).await;
    step_confirm(&mut reader, &mut client_write).await;
    step_tool_result(&mut reader, &mut client_write).await;
}
