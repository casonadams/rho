use super::discovery::{extract_project_id, is_selectable_runtime_model};
use super::http::{antigravity_headers, friendly_error};
use rig::completion::CompletionRequest;

#[test]
fn is_selectable_runtime_model_filters_correctly() {
    assert!(is_selectable_runtime_model("gemini-2.5-pro"));
    assert!(is_selectable_runtime_model("gemini-3.7-flash"));
    assert!(is_selectable_runtime_model("claude-sonnet-4-6"));
    assert!(is_selectable_runtime_model("gpt-oss-1"));

    // Excluded patterns
    assert!(!is_selectable_runtime_model("gemini-image-gen"));
    assert!(!is_selectable_runtime_model("gemini-2.5 chat"));
    assert!(!is_selectable_runtime_model("MODEL_GEMINI_1"));
    assert!(!is_selectable_runtime_model("text-embedding-004"));
    assert!(!is_selectable_runtime_model("chat-bison-001"));
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
fn friendly_error_formats_known_error_cases() {
    let quota_body = r#"{"error":{"message":"Individual quota reached. Resets in 2h45m."}}"#;
    let quota_err = friendly_error(Some(429), quota_body);
    assert!(quota_err.contains("Resets in 2h45m"));

    let rate_limit_body = r#"{"error":{"message":"Resource has been exhausted (e.g. check quota)."}}"#;
    let rate_err = friendly_error(Some(429), rate_limit_body);
    assert!(rate_err.contains("rate limit reached"));

    let auth_err = friendly_error(Some(401), "Unauthorized");
    assert!(auth_err.contains("rho login antigravity"));

    let forbidden = friendly_error(Some(403), r#"{"error":{"message":"Permission denied"}}"#);
    assert!(forbidden.contains("access denied"));
    assert!(forbidden.contains("Permission denied"));

    let not_found = friendly_error(Some(404), r#"{"error":{"message":"Model not found"}}"#);
    assert!(not_found.contains("Model not available"));

    let capacity = friendly_error(Some(503), r#"{"error":{"message":"No capacity available"}}"#);
    assert!(capacity.contains("no capacity right now"));

    let generic_500 = friendly_error(Some(500), r#"{"error":{"message":"Internal server error"}}"#);
    assert!(generic_500.contains("API error (500)"));

    let none_status = friendly_error(None, "Connection closed");
    assert!(none_status.contains("Antigravity request failed: Connection closed"));
}

#[test]
fn antigravity_headers_sets_expected_keys() {
    let headers = antigravity_headers("test-secret-token");
    assert_eq!(
        headers.get("authorization").and_then(|v| v.to_str().ok()),
        Some("Bearer test-secret-token")
    );
    assert_eq!(
        headers.get("content-type").and_then(|v| v.to_str().ok()),
        Some("application/json")
    );
    assert!(headers.get("user-agent").is_some());
    assert!(headers.get("x-goog-api-client").is_some());
    assert!(headers.get("client-metadata").is_some());
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

#[tokio::test]
async fn open_stream_retries_on_401_with_forced_token_refresh() {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        // Request 1: 401 Unauthorized
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let resp = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\nConnection: close\r\n\r\nUnauthorized";
            let _ = stream.write_all(resp.as_bytes()).await;
        }
        // Request 2: 200 OK
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).await.unwrap_or(0);
            let req_str = String::from_utf8_lossy(&buf[..n]);
            assert!(req_str.contains("Bearer token-refreshed"));
            let resp = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 11\r\nConnection: close\r\n\r\ndata: {}\n\n";
            let _ = stream.write_all(resp.as_bytes()).await;
        }
    });

    struct MockProvider {
        refresh_count: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl TokenProvider for MockProvider {
        async fn token(&self) -> Result<String, String> {
            Ok("token-initial".into())
        }
        async fn force_refresh(&self) -> Result<String, String> {
            self.refresh_count.fetch_add(1, Ordering::SeqCst);
            Ok("token-refreshed".into())
        }
    }

    let refresh_count = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(MockProvider {
        refresh_count: refresh_count.clone(),
    });

    let client = AntigravityClient::with_token_provider(provider, "test-project", "gemini-2.5-pro")
        .with_endpoint(format!("http://{addr}"));

    let req = test_completion_request();
    let response = client.open_stream(&req).await;
    assert!(response.is_ok(), "Expected retry to succeed with 200 OK");
    assert_eq!(refresh_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn open_stream_stops_after_single_retry_if_401_persists() {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        // Request 1: 401 Unauthorized
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let resp = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\nConnection: close\r\n\r\nUnauthorized";
            let _ = stream.write_all(resp.as_bytes()).await;
        }
        // Request 2 (retry): 401 Unauthorized again
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let resp = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\nConnection: close\r\n\r\nUnauthorized";
            let _ = stream.write_all(resp.as_bytes()).await;
        }
    });

    struct MockProvider {
        refresh_count: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl TokenProvider for MockProvider {
        async fn token(&self) -> Result<String, String> {
            Ok("token-1".into())
        }
        async fn force_refresh(&self) -> Result<String, String> {
            self.refresh_count.fetch_add(1, Ordering::SeqCst);
            Ok("token-2".into())
        }
    }

    let refresh_count = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(MockProvider {
        refresh_count: refresh_count.clone(),
    });

    let client = AntigravityClient::with_token_provider(provider, "test-project", "gemini-2.5-pro")
        .with_endpoint(format!("http://{addr}"));

    let req = test_completion_request();
    let err = client.open_stream(&req).await.unwrap_err();
    assert_eq!(err.0, Some(401));
    assert_eq!(refresh_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn open_stream_fails_immediately_if_refresh_fails() {
    use super::*;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let resp = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\nConnection: close\r\n\r\nUnauthorized";
            let _ = stream.write_all(resp.as_bytes()).await;
        }
    });

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

    let client =
        AntigravityClient::with_token_provider(Arc::new(FailingRefreshProvider), "test-project", "gemini-2.5-pro")
            .with_endpoint(format!("http://{addr}"));

    let req = test_completion_request();
    let err = client.open_stream(&req).await.unwrap_err();
    assert_eq!(err.0, Some(401));
}
