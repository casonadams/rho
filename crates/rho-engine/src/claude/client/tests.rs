use super::*;
use crate::claude::http::{claude_headers, friendly_error};
use crate::claude::request::{build_request_body, normalize_model_alias, resolve_thinking_budget};
use rig::completion::ToolDefinition;
use rig::message::{
    AssistantContent, Message, Text, ToolCall, ToolCallId, ToolFunction, ToolResult, ToolResultContent, UserContent,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn sample_request() -> CompletionRequest {
    CompletionRequest {
        model: None,
        preamble: Some("system instructions".to_string()),
        chat_history: vec![Message::user("hello world")],
        documents: Vec::new(),
        tools: Vec::new(),
        temperature: Some(0.7),
        max_tokens: None,
        tool_choice: None,
        additional_params: None,
        output_schema: None,
        record_telemetry_content: false,
    }
}

#[test]
fn test_model_alias_normalization() {
    let cases = [
        ("default", "claude-sonnet-4-6"),
        ("sonnet", "claude-sonnet-4-6"),
        ("claude-sonnet-4-6", "claude-sonnet-4-6"),
        ("claude-sonnet-4-5", "claude-sonnet-4-5-20250514"),
        ("opus", "claude-opus-4-6"),
        ("claude-opus-4-6", "claude-opus-4-6"),
        ("haiku", "claude-haiku-4-5"),
        ("claude-haiku-4-5", "claude-haiku-4-5"),
        ("sonnet-5", "claude-sonnet-5"),
        ("claude-sonnet-5", "claude-sonnet-5"),
        ("opus-5", "claude-opus-5"),
        ("claude-opus-5", "claude-opus-5"),
        ("claude-3-7-sonnet-20250219", "claude-3-7-sonnet-20250219"),
    ];
    for (alias, expected) in cases {
        assert_eq!(normalize_model_alias(alias), expected);
    }
}

#[test]
fn test_thinking_budget_resolution() {
    let cases = [
        (Some("minimal"), Some(1024)),
        (Some("low"), Some(2048)),
        (Some("medium"), Some(4096)),
        (Some("high"), Some(16384)),
        (Some("xhigh"), Some(16384)),
        (Some("max"), Some(16384)),
        (Some("off"), None),
        (None, None),
    ];
    for (input, expected) in cases {
        assert_eq!(resolve_thinking_budget(input), expected);
    }
}

#[test]
fn test_build_request_body_with_thinking_omits_temperature() {
    let req = sample_request();
    let body = build_request_body("claude-sonnet-4-5", Some("medium"), &req).unwrap();
    let actual = (
        body["model"].as_str(),
        body["system"][0]["text"].as_str(),
        body["system"][0]["cache_control"]["type"].as_str(),
        body["thinking"]["type"].as_str(),
        body["thinking"]["budget_tokens"].as_u64(),
    );
    assert_eq!(
        actual,
        (
            Some("claude-sonnet-4-5-20250514"),
            Some("system instructions"),
            Some("ephemeral"),
            Some("enabled"),
            Some(4096)
        )
    );
    assert!(body.get("temperature").is_none() && body["max_tokens"].as_u64().unwrap() >= 8192);
}

#[test]
fn test_build_request_body_without_thinking_includes_temperature() {
    let req = sample_request();
    let body = build_request_body("claude-haiku-4-5", None, &req).unwrap();

    assert_eq!(body["model"], "claude-haiku-4-5");
    assert!(body.get("thinking").is_none());
    assert_eq!(body["temperature"], 0.7);
    assert_eq!(body["max_tokens"], 8192);
}

fn sample_tool_history_chat(call: ToolCall, res: ToolResult) -> Vec<Message> {
    vec![
        Message::user("run tool"),
        Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(call)],
        },
        Message::User {
            content: vec![UserContent::ToolResult(res)],
        },
    ]
}

fn request_with_tool_and_history() -> CompletionRequest {
    let mut req = sample_request();
    req.tools.push(ToolDefinition {
        name: "test_tool".into(),
        description: "A test tool".into(),
        parameters: serde_json::json!({ "type": "object", "properties": { "arg": { "type": "string" } } }),
    });
    let call = ToolCall::new(
        ToolCallId::new("tool_call_1").unwrap(),
        ToolFunction::new("test_tool".into(), serde_json::json!({ "arg": "val" })),
    );
    let res = ToolResult {
        call: ToolCallId::new("tool_call_1").unwrap(),
        provider: None,
        name: "test_tool".into(),
        content: vec![ToolResultContent::Text(Text::new("tool output"))],
    };
    req.chat_history = sample_tool_history_chat(call, res);
    req
}

#[test]
fn test_build_request_body_converts_messages_and_tools() {
    let req = request_with_tool_and_history();
    let body = build_request_body("default", None, &req).unwrap();
    let messages = body["messages"].as_array().unwrap();
    let roles = (
        messages.len(),
        messages[0]["role"].as_str(),
        messages[1]["role"].as_str(),
    );
    assert_eq!(roles, (3, Some("user"), Some("assistant")));
    let types = (
        messages[1]["content"][0]["type"].as_str(),
        messages[2]["content"][0]["type"].as_str(),
    );
    assert_eq!(types, (Some("tool_use"), Some("tool_result")));

    let tools = body["tools"].as_array().unwrap();
    let tool_info = (
        tools.len(),
        tools[0]["name"].as_str(),
        tools[0]["input_schema"]["type"].as_str(),
    );
    assert_eq!(tool_info, (1, Some("mcp__rho__test_tool"), Some("object")));
}

#[test]
fn test_build_request_body_marks_cache_breakpoints() {
    let req = request_with_tool_and_history();
    let body = build_request_body("default", None, &req).unwrap();

    assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
    assert_eq!(body["tools"][0]["cache_control"]["type"], "ephemeral");
    let messages = body["messages"].as_array().unwrap();
    let tool_result = messages
        .iter()
        .rev()
        .find_map(|m| {
            m["content"]
                .as_array()
                .and_then(|parts| parts.iter().find(|p| p["type"] == "tool_result"))
        })
        .unwrap();
    assert_eq!(tool_result["cache_control"]["type"], "ephemeral");
}

#[test]
fn test_build_request_body_leaves_history_uncached_without_tool_results() {
    let req = sample_request();
    let body = build_request_body("default", None, &req).unwrap();

    for message in body["messages"].as_array().unwrap() {
        for part in message["content"].as_array().unwrap() {
            assert!(part.get("cache_control").is_none());
        }
    }
}

#[test]
fn test_claude_headers_contains_required_fields() {
    let headers = claude_headers("test-token-xyz");
    assert_eq!(headers.get("authorization").unwrap(), "Bearer test-token-xyz");
    assert_eq!(headers.get("anthropic-version").unwrap(), "2023-06-01");
    assert_eq!(
        headers.get("anthropic-beta").unwrap(),
        "claude-code-20250219,oauth-2025-04-20"
    );
    assert_eq!(headers.get("user-agent").unwrap(), "claude-cli/2.1.226 (external, cli)");
}

#[test]
fn test_friendly_error_formatting() {
    assert!(friendly_error(Some(401), "").contains("Run 'rho login claude'"));
    assert!(friendly_error(Some(429), r#"{"error":{"message":"over limit"}}"#).contains("over limit"));
    assert!(friendly_error(Some(529), "").contains("overloaded"));
}

struct MockProvider {
    token_val: String,
    refresh_count: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl TokenProvider for MockProvider {
    async fn token(&self) -> Result<String, String> {
        Ok(self.token_val.clone())
    }
    async fn force_refresh(&self) -> Result<String, String> {
        self.refresh_count.fetch_add(1, Ordering::SeqCst);
        Ok("refreshed-token".into())
    }
}

async fn spawn_two_responses_server(resp1: &'static str, resp2: &'static str) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        for resp in [resp1, resp2] {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf).await;
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        }
    });
    addr
}

#[tokio::test]
async fn test_open_stream_retries_on_401_with_forced_token_refresh() {
    let r401 = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\nConnection: close\r\n\r\nUnauthorized";
    let r200 = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"type\": \"message_stop\"}\n\n";
    let addr = spawn_two_responses_server(r401, r200).await;

    let refresh_count = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(MockProvider {
        token_val: "stale-token".into(),
        refresh_count: refresh_count.clone(),
    });
    let client =
        ClaudeClient::with_token_provider(provider, "claude-sonnet-4-5").with_endpoint(format!("http://{addr}"));

    assert!(client.open_stream(&sample_request()).await.is_ok());
    assert_eq!(refresh_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_open_stream_stops_after_single_retry_if_401_persists() {
    let r401 = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\nConnection: close\r\n\r\nUnauthorized";
    let addr = spawn_two_responses_server(r401, r401).await;

    let refresh_count = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(MockProvider {
        token_val: "stale-token".into(),
        refresh_count: refresh_count.clone(),
    });
    let client =
        ClaudeClient::with_token_provider(provider, "claude-sonnet-4-5").with_endpoint(format!("http://{addr}"));

    let err = client.open_stream(&sample_request()).await.unwrap_err();
    assert_eq!((err.0, refresh_count.load(Ordering::SeqCst)), (Some(401), 1));
}

async fn spawn_sse_server(sse_body: &str) -> std::net::SocketAddr {
    let resp_str = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse_body}",
        sse_body.len()
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let _ = stream.write_all(resp_str.as_bytes()).await;
        }
    });
    addr
}

#[tokio::test]
async fn test_completion_model_aggregates_unary_response() {
    use rig::completion::CompletionModel;
    let sse_body = "data: {\"type\": \"message_start\", \"message\": {\"id\": \"msg_1\", \"usage\": {\"input_tokens\": 12}}}\n\ndata: {\"type\": \"content_block_start\", \"index\": 0, \"content_block\": {\"type\": \"text\"}}\n\ndata: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"type\": \"text_delta\", \"text\": \"Full response text\"}}\n\ndata: {\"type\": \"content_block_stop\", \"index\": 0}\n\ndata: {\"type\": \"message_delta\", \"delta\": {\"stop_reason\": \"end_turn\"}, \"usage\": {\"output_tokens\": 8}}\n\ndata: {\"type\": \"message_stop\"}\n\n";
    let addr = spawn_sse_server(sse_body).await;

    let client = ClaudeClient::new("test-token", "claude-sonnet-4-5").with_endpoint(format!("http://{addr}"));
    let resp = client.completion(sample_request()).await.unwrap();
    assert_eq!(
        (
            resp.usage.input_tokens,
            resp.usage.output_tokens,
            resp.usage.total_tokens
        ),
        (12, 8, 20)
    );
    assert!(
        resp.choice
            .iter()
            .any(|c| matches!(c, AssistantContent::Text(t) if t.text == "Full response text"))
    );
}
