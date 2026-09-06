use super::super::AntigravityClient;
use super::discovery::{extract_project_id, is_selectable_runtime_model};
use super::http::{antigravity_headers, friendly_error};
use crate::auth::TokenProvider;
use rig::completion::CompletionRequest;

#[test]
fn is_selectable_runtime_model_filters_correctly() {
    for model in ["gemini-2.5-pro", "gemini-3.7-flash", "claude-sonnet-4-6", "gpt-oss-1"] {
        assert!(is_selectable_runtime_model(model));
    }
    for model in [
        "gemini-image-gen",
        "gemini-2.5 chat",
        "MODEL_GEMINI_1",
        "text-embedding-004",
        "chat-bison-001",
    ] {
        assert!(!is_selectable_runtime_model(model));
    }
}

#[test]
fn extract_project_id_from_direct_fields() {
    let json1 = serde_json::json!({ "antigravityProjectId": "proj-anti-1" });
    assert_eq!(extract_project_id(&json1), Some("proj-anti-1".to_string()));

    let json2 = serde_json::json!({ "projectId": "proj-2" });
    assert_eq!(extract_project_id(&json2), Some("proj-2".to_string()));

    let json3 = serde_json::json!({ "backendProjectId": "proj-3" });
    assert_eq!(extract_project_id(&json3), Some("proj-3".to_string()));

    let json4 = serde_json::json!({ "cloudaicompanionProject": "proj-4" });
    assert_eq!(extract_project_id(&json4), Some("proj-4".to_string()));
}

#[test]
fn extract_project_id_from_nested_arrays() {
    let json_str_array = serde_json::json!({
        "projects": ["first-proj", "second-proj"]
    });
    assert_eq!(extract_project_id(&json_str_array), Some("first-proj".to_string()));

    let json_nested_obj = serde_json::json!({
        "cloudaicompanionProjects": [
            { "projectId": "nested-proj" }
        ]
    });
    assert_eq!(extract_project_id(&json_nested_obj), Some("nested-proj".to_string()));

    let json_empty = serde_json::json!({});
    assert_eq!(extract_project_id(&json_empty), None);
}

#[test]
fn friendly_error_formats_quota_cases() {
    let cases = [
        (
            Some(429),
            r#"{"error":{"message":"Individual quota reached. Resets in 2h45m."}}"#,
            "Resets in 2h45m",
        ),
        (
            Some(429),
            r#"{"error":{"message":"Resource has been exhausted (e.g. check quota)."}}"#,
            "rate limit reached",
        ),
    ];
    for (status, body, expected) in cases {
        assert!(friendly_error(status, body).contains(expected));
    }
}

#[test]
fn friendly_error_formats_auth_and_not_found_cases() {
    let cases = [
        (Some(401), "Unauthorized", "rho login antigravity"),
        (
            Some(403),
            r#"{"error":{"message":"Permission denied"}}"#,
            "access denied",
        ),
        (
            Some(404),
            r#"{"error":{"message":"Model not found"}}"#,
            "Model not available",
        ),
    ];
    for (status, body, expected) in cases {
        assert!(friendly_error(status, body).contains(expected));
    }
}

#[test]
fn friendly_error_formats_server_and_network_error_cases() {
    let cases = [
        (
            Some(503),
            r#"{"error":{"message":"No capacity available"}}"#,
            "no capacity right now",
        ),
        (
            Some(500),
            r#"{"error":{"message":"Internal server error"}}"#,
            "API error (500)",
        ),
        (
            None,
            "Connection closed",
            "Antigravity request failed: Connection closed",
        ),
    ];
    for (status, body, expected) in cases {
        assert!(friendly_error(status, body).contains(expected));
    }
}

#[test]
fn antigravity_headers_sets_expected_keys() {
    let headers = antigravity_headers("test-secret-token");
    let actual = (
        headers.get("authorization").and_then(|v| v.to_str().ok()),
        headers.get("content-type").and_then(|v| v.to_str().ok()),
    );
    assert_eq!(actual, (Some("Bearer test-secret-token"), Some("application/json")));
    for key in ["user-agent", "x-goog-api-client", "client-metadata"] {
        assert!(headers.get(key).is_some());
    }
}

fn test_completion_request() -> CompletionRequest {
    CompletionRequest {
        model: None,
        preamble: Some("system prompt".to_string()),
        chat_history: vec![rig::message::Message::user("hello")],
        documents: Vec::new(),
        tools: Vec::new(),
        temperature: None,
        max_tokens: None,
        tool_choice: None,
        additional_params: None,
        output_schema: None,
        record_telemetry_content: false,
    }
}

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

struct MockTokenProvider {
    token_val: String,
    refresh_count: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl TokenProvider for MockTokenProvider {
    async fn token(&self) -> Result<String, String> {
        Ok(self.token_val.clone())
    }
    async fn force_refresh(&self) -> Result<String, String> {
        self.refresh_count.fetch_add(1, Ordering::SeqCst);
        Ok("token-refreshed".into())
    }
}

struct FailingRefreshProvider;

#[async_trait::async_trait]
impl TokenProvider for FailingRefreshProvider {
    async fn token(&self) -> Result<String, String> {
        Ok("stale-token".into())
    }
    async fn force_refresh(&self) -> Result<String, String> {
        Err("token revoked".into())
    }
}

async fn spawn_two_responses(resp1: &'static str, resp2: &'static str) -> std::net::SocketAddr {
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
async fn open_stream_retries_on_401_with_forced_token_refresh() {
    let r401 = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\nConnection: close\r\n\r\nUnauthorized";
    let r200 = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 11\r\nConnection: close\r\n\r\ndata: {}\n\n";
    let addr = spawn_two_responses(r401, r200).await;

    let refresh_count = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(MockTokenProvider {
        token_val: "token-initial".into(),
        refresh_count: refresh_count.clone(),
    });
    let client = AntigravityClient::with_token_provider(provider, "test-project", "gemini-2.5-pro")
        .with_endpoint(format!("http://{addr}"));

    assert!(client.open_stream(&test_completion_request()).await.is_ok());
    assert_eq!(refresh_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn open_stream_stops_after_single_retry_if_401_persists() {
    let r401 = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\nConnection: close\r\n\r\nUnauthorized";
    let addr = spawn_two_responses(r401, r401).await;

    let refresh_count = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(MockTokenProvider {
        token_val: "token-1".into(),
        refresh_count: refresh_count.clone(),
    });
    let client = AntigravityClient::with_token_provider(provider, "test-project", "gemini-2.5-pro")
        .with_endpoint(format!("http://{addr}"));

    let err = client.open_stream(&test_completion_request()).await.unwrap_err();
    assert_eq!((err.0, refresh_count.load(Ordering::SeqCst)), (Some(401), 1));
}

#[tokio::test]
async fn open_stream_fails_immediately_if_refresh_fails() {
    let r401 = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\nConnection: close\r\n\r\nUnauthorized";
    let addr = spawn_two_responses(r401, r401).await;

    let client =
        AntigravityClient::with_token_provider(Arc::new(FailingRefreshProvider), "test-project", "gemini-2.5-pro")
            .with_endpoint(format!("http://{addr}"));
    let err = client.open_stream(&test_completion_request()).await.unwrap_err();
    assert_eq!(err.0, Some(401));
}
