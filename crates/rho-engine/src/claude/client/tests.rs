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
    assert_eq!(normalize_model_alias("default"), "claude-sonnet-4-5-20250514");
    assert_eq!(normalize_model_alias("sonnet"), "claude-sonnet-4-5-20250514");
    assert_eq!(normalize_model_alias("claude-sonnet-4-5"), "claude-sonnet-4-5-20250514");
    assert_eq!(normalize_model_alias("opus"), "claude-opus-4-6");
    assert_eq!(normalize_model_alias("claude-opus-4-6"), "claude-opus-4-6");
    assert_eq!(normalize_model_alias("haiku"), "claude-haiku-4-5");
    assert_eq!(normalize_model_alias("claude-haiku-4-5"), "claude-haiku-4-5");
    assert_eq!(
        normalize_model_alias("claude-3-7-sonnet-20250219"),
        "claude-3-7-sonnet-20250219"
    );
}

#[test]
fn test_thinking_budget_resolution() {
    assert_eq!(resolve_thinking_budget(Some("minimal")), Some(1024));
    assert_eq!(resolve_thinking_budget(Some("low")), Some(2048));
    assert_eq!(resolve_thinking_budget(Some("medium")), Some(4096));
    assert_eq!(resolve_thinking_budget(Some("high")), Some(16384));
    assert_eq!(resolve_thinking_budget(Some("xhigh")), Some(16384));
    assert_eq!(resolve_thinking_budget(Some("max")), Some(16384));
    assert_eq!(resolve_thinking_budget(Some("off")), None);
    assert_eq!(resolve_thinking_budget(None), None);
}

#[test]
fn test_build_request_body_with_thinking_omits_temperature() {
    let req = sample_request();
    let body = build_request_body("claude-sonnet-4-5", Some("medium"), &req).unwrap();

    assert_eq!(body["model"], "claude-sonnet-4-5-20250514");
    assert_eq!(body["system"], "system instructions");
    assert_eq!(body["thinking"]["type"], "enabled");
    assert_eq!(body["thinking"]["budget_tokens"], 4096);
    assert!(body.get("temperature").is_none());
    assert!(body["max_tokens"].as_u64().unwrap() >= 8192);
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

#[test]
fn test_build_request_body_converts_messages_and_tools() {
    let mut req = sample_request();
    req.tools.push(ToolDefinition {
        name: "test_tool".into(),
        description: "A test tool".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": { "arg": { "type": "string" } }
        }),
    });
    req.chat_history = vec![
        Message::user("run tool"),
        Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(ToolCall {
                id: ToolCallId::new("tool_call_1").unwrap(),
                provider: None,
                function: ToolFunction {
                    name: "test_tool".into(),
                    arguments: serde_json::json!({ "arg": "val" }),
                },
                signature: None,
                additional_params: None,
            })],
        },
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: ToolCallId::new("tool_call_1").unwrap(),
                provider: None,
                name: "test_tool".into(),
                content: vec![ToolResultContent::Text(Text::new("tool output"))],
            })],
        },
    ];

    let body = build_request_body("default", None, &req).unwrap();
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"][0]["type"], "tool_use");
    assert_eq!(messages[2]["role"], "user");
    assert_eq!(messages[2]["content"][0]["type"], "tool_result");

    let tools = body["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "test_tool");
    assert_eq!(tools[0]["input_schema"]["type"], "object");
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
    assert_eq!(headers.get("user-agent").unwrap(), "claude-cli/2.1.62");
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

#[tokio::test]
async fn test_open_stream_retries_on_401_with_forced_token_refresh() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        // First request: 401 Unauthorized
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let resp = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\nConnection: close\r\n\r\nUnauthorized";
            let _ = stream.write_all(resp.as_bytes()).await;
        }
        // Second request (retry): 200 OK
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let resp = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"type\": \"message_stop\"}\n\n";
            let _ = stream.write_all(resp.as_bytes()).await;
        }
    });

    let refresh_count = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(MockProvider {
        token_val: "stale-token".into(),
        refresh_count: refresh_count.clone(),
    });

    let client =
        ClaudeClient::with_token_provider(provider, "claude-sonnet-4-5").with_endpoint(format!("http://{addr}"));

    let req = sample_request();
    let res = client.open_stream(&req).await;
    assert!(res.is_ok());
    assert_eq!(refresh_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_open_stream_stops_after_single_retry_if_401_persists() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        // First request: 401
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let resp = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\nConnection: close\r\n\r\nUnauthorized";
            let _ = stream.write_all(resp.as_bytes()).await;
        }
        // Second request: 401 again
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let resp = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\nConnection: close\r\n\r\nUnauthorized";
            let _ = stream.write_all(resp.as_bytes()).await;
        }
    });

    let refresh_count = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(MockProvider {
        token_val: "stale-token".into(),
        refresh_count: refresh_count.clone(),
    });

    let client =
        ClaudeClient::with_token_provider(provider, "claude-sonnet-4-5").with_endpoint(format!("http://{addr}"));

    let req = sample_request();
    let err = client.open_stream(&req).await.unwrap_err();
    assert_eq!(err.0, Some(401));
    assert_eq!(refresh_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_completion_model_aggregates_unary_response() {
    use rig::completion::CompletionModel;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let sse_body = "\
data: {\"type\": \"message_start\", \"message\": {\"id\": \"msg_1\", \"usage\": {\"input_tokens\": 12}}}\n\n\
data: {\"type\": \"content_block_start\", \"index\": 0, \"content_block\": {\"type\": \"text\"}}\n\n\
data: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"type\": \"text_delta\", \"text\": \"Full response text\"}}\n\n\
data: {\"type\": \"content_block_stop\", \"index\": 0}\n\n\
data: {\"type\": \"message_delta\", \"delta\": {\"stop_reason\": \"end_turn\"}, \"usage\": {\"output_tokens\": 8}}\n\n\
data: {\"type\": \"message_stop\"}\n\n";

            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                sse_body.len(),
                sse_body
            );
            let _ = stream.write_all(resp.as_bytes()).await;
        }
    });

    let client = ClaudeClient::new("test-token", "claude-sonnet-4-5").with_endpoint(format!("http://{addr}"));

    let req = sample_request();
    let resp = client.completion(req).await.unwrap();
    assert_eq!(resp.usage.input_tokens, 12);
    assert_eq!(resp.usage.output_tokens, 8);
    assert_eq!(resp.usage.total_tokens, 20);
    assert!(
        resp.choice
            .iter()
            .any(|c| matches!(c, AssistantContent::Text(t) if t.text == "Full response text"))
    );
}
