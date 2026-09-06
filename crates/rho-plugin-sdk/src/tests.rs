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

async fn send_line(writer: &mut ClientWriter, line: &str) {
    writer.write_all(line.as_bytes()).await.unwrap();
}

async fn read_json(reader: &mut ClientReader) -> Value {
    let line = reader.next_line().await.unwrap().unwrap();
    serde_json::from_str(&line).unwrap()
}

async fn step_init(reader: &mut ClientReader, writer: &mut ClientWriter) {
    send_line(writer, "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n").await;
    let val = read_json(reader).await;
    assert_eq!(val["id"], 1);
    assert_eq!(val["result"]["serverInfo"]["name"], "test_guard");
}

async fn step_allow(reader: &mut ClientReader, writer: &mut ClientWriter) {
    send_line(
        writer,
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"hook/tool_call\",\"params\":{\"event\":\"tool_call\",\"tool_name\":\"bash\",\"args\":{\"command\":\"ls\"}}}\n",
    )
    .await;
    let val = read_json(reader).await;
    assert_eq!(val["id"], 2);
    assert_eq!(val["result"]["action"], "continue");
}

fn check_deny(deny: &Value) {
    assert_eq!(deny["id"], 3);
    assert_eq!(deny["result"]["action"], "skip");
    assert_eq!(deny["result"]["reason"], "Blocked root deletion");
}

async fn step_deny(reader: &mut ClientReader, writer: &mut ClientWriter) {
    send_line(
        writer,
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"hook/tool_call\",\"params\":{\"event\":\"tool_call\",\"tool_name\":\"bash\",\"args\":{\"command\":\"rm -rf /\"}}}\n",
    )
    .await;
    check_deny(&read_json(reader).await);
}

async fn reply_confirm(writer: &mut ClientWriter, req: &Value) {
    assert_eq!(req["method"], "host/ui/confirm");
    let req_id = req["id"].as_u64().unwrap();
    let reply = json!({"jsonrpc": "2.0", "id": req_id, "result": {"confirmed": false}});
    send_line(writer, &format!("{reply}\n")).await;
}

fn check_confirm_res(res: &Value) {
    assert_eq!(res["id"], 4);
    assert_eq!(res["result"]["action"], "skip");
    assert_eq!(res["result"]["reason"], "Reboot denied by user");
}

async fn step_confirm(reader: &mut ClientReader, writer: &mut ClientWriter) {
    send_line(
        writer,
        "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"hook/tool_call\",\"params\":{\"event\":\"tool_call\",\"tool_name\":\"bash\",\"args\":{\"command\":\"sudo reboot\"}}}\n",
    )
    .await;
    reply_confirm(writer, &read_json(reader).await).await;
    check_confirm_res(&read_json(reader).await);
}

async fn respond_success(writer: &mut ClientWriter, id: Option<u64>) {
    let resp = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{{\"success\":true}}}}\n",
        id.unwrap()
    );
    send_line(writer, &resp).await;
}

async fn handle_tool_ui_block(reader: &mut ClientReader, writer: &mut ClientWriter) {
    let block = read_json(reader).await;
    assert_eq!(block["method"], "host/ui/block");
    respond_success(writer, block["id"].as_u64()).await;
}

async fn handle_tool_ui_status(reader: &mut ClientReader, writer: &mut ClientWriter) {
    let status = read_json(reader).await;
    assert_eq!(status["method"], "host/ui/set_status");
    respond_success(writer, status["id"].as_u64()).await;
}

fn check_tool_res(res: &Value) {
    assert_eq!(res["id"], 5);
    assert_eq!(res["result"]["action"], "continue");
}

async fn step_tool_result(reader: &mut ClientReader, writer: &mut ClientWriter) {
    send_line(
        writer,
        "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"hook/tool_result\",\"params\":{\"event\":\"tool_result\",\"tool_name\":\"bash\",\"args\":{},\"output\":\"ok\",\"is_error\":false}}\n",
    )
    .await;
    handle_tool_ui_block(reader, writer).await;
    handle_tool_ui_status(reader, writer).await;
    check_tool_res(&read_json(reader).await);
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
    step_allow(&mut reader, &mut client_write).await;
    step_deny(&mut reader, &mut client_write).await;
    step_confirm(&mut reader, &mut client_write).await;
    step_tool_result(&mut reader, &mut client_write).await;
}
