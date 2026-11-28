use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub enum Reply {
    Result(Value),
    /// Server-sent events emitted before the JSON-RPC result, e.g. notifications.
    SseWith(Vec<Value>, Value),
    /// Closes the connection without answering, like a crashed server.
    Drop,
}

pub type Handler = Arc<dyn Fn(&str, &Value) -> futures::future::BoxFuture<'static, Reply> + Send + Sync>;

#[derive(Clone)]
pub struct FakeServer {
    pub url: String,
    methods: Arc<Mutex<Vec<String>>>,
}

impl FakeServer {
    pub fn methods(&self) -> Vec<String> {
        self.methods.lock().unwrap().clone()
    }
}

pub fn handler<F, Fut>(f: F) -> Handler
where
    F: Fn(String, Value) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Reply> + Send + 'static,
{
    Arc::new(move |method, params| Box::pin(f(method.to_string(), params.clone())))
}

pub fn init_result(instructions: Option<&str>) -> Reply {
    let mut result = json!({
        "protocolVersion": "2025-11-25",
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "fake" }
    });
    if let Some(text) = instructions {
        result["instructions"] = json!(text);
    }
    Reply::Result(result)
}

pub fn tools_result(names: &[&str]) -> Reply {
    let tools: Vec<Value> = names
        .iter()
        .map(|n| json!({ "name": n, "description": format!("{n} tool"), "inputSchema": { "type": "object" } }))
        .collect();
    Reply::Result(json!({ "tools": tools }))
}

pub async fn read_request(socket: &mut TcpStream) -> Option<Value> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = socket.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        let text = String::from_utf8_lossy(&buf);
        let Some(header_end) = text.find("\r\n\r\n") else {
            continue;
        };
        let length = text[..header_end]
            .lines()
            .find_map(|l| {
                l.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(|v| v.trim().to_string())
            })
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        if buf.len() >= header_end + 4 + length {
            return serde_json::from_slice(&buf[header_end + 4..header_end + 4 + length]).ok();
        }
    }
}

fn http_response(content_type: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

async fn serve(mut socket: TcpStream, handler: Handler, methods: Arc<Mutex<Vec<String>>>) {
    let Some(request) = read_request(&mut socket).await else {
        return;
    };
    let method = request["method"].as_str().unwrap_or_default().to_string();
    methods.lock().unwrap().push(method.clone());
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    let Some(id) = request.get("id").cloned() else {
        handler(&method, &params).await;
        let _ = socket
            .write_all(b"HTTP/1.1 202 Accepted\r\nConnection: close\r\nContent-Length: 0\r\n\r\n")
            .await;
        return;
    };
    let response = match handler(&method, &params).await {
        Reply::Result(result) => {
            let body = json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string();
            http_response("application/json", &body)
        }
        Reply::SseWith(events, result) => {
            let mut body = String::new();
            for event in events
                .iter()
                .chain([&json!({ "jsonrpc": "2.0", "id": id, "result": result })])
            {
                body.push_str(&format!("event: message\ndata: {event}\n\n"));
            }
            http_response("text/event-stream", &body)
        }
        Reply::Drop => return,
    };
    let _ = socket.write_all(response.as_bytes()).await;
}

pub async fn spawn(handler: Handler) -> FakeServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let methods = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&methods);
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(serve(socket, Arc::clone(&handler), Arc::clone(&seen)));
        }
    });
    FakeServer { url, methods }
}

pub async fn eventually(what: &str, condition: impl Fn() -> bool) {
    let waited = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !condition() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await;
    assert!(waited.is_ok(), "timed out waiting for {what}");
}
