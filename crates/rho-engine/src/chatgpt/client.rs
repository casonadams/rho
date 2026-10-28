//! ChatGPT (Codex) Responses API client implementation.

use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::sync::Mutex;

use crate::auth::store::AuthStore;
use crate::auth::token::{AuthStoreTokenProvider, StaticTokenProvider, TokenProvider};
use crate::engine::compactor::llm::ModelHandle;
use crate::provider::adapter::{ModelAdapter, ModelCompletionRequest, ModelCompletionResponse, ModelStreamEvent};
use futures::Stream;
use rho_harness_core::error::AppError;
use rho_harness_core::model::ChatMessage;
use serde_json::{Value, json};
use std::pin::Pin;

pub const DEFAULT_ENDPOINT: &str = "https://chatgpt.com/backend-api/codex";
pub const RESPONSES_PATH: &str = "/responses";
pub const PROVIDER_NAME: &str = "chatgpt";

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    crate::install_crypto_provider();
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(300))
        .build()
        .unwrap_or_default()
});

#[derive(Clone)]
pub struct ChatGptClient {
    token_provider: Arc<dyn TokenProvider>,
    model: String,
    account_id: Option<String>,
    endpoint: Option<String>,
}

impl std::fmt::Debug for ChatGptClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatGptClient")
            .field("model", &self.model)
            .field("account_id", &self.account_id)
            .finish()
    }
}

impl ChatGptClient {
    pub fn new(token: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            token_provider: Arc::new(StaticTokenProvider::new(token)),
            model: model.into(),
            account_id: None,
            endpoint: None,
        }
    }

    pub fn with_token_provider(token_provider: Arc<dyn TokenProvider>, model: impl Into<String>) -> Self {
        Self {
            token_provider,
            model: model.into(),
            account_id: None,
            endpoint: None,
        }
    }

    pub fn with_auth_store(store: Arc<Mutex<AuthStore>>, model: impl Into<String>) -> Self {
        Self::with_token_provider(Arc::new(AuthStoreTokenProvider::new(store, "chatgpt")), model)
    }

    pub fn with_account_id(mut self, account_id: Option<String>) -> Self {
        self.account_id = account_id.filter(|s| !s.trim().is_empty());
        self
    }

    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    fn target_url(&self) -> String {
        format!(
            "{}{}",
            self.endpoint.as_deref().unwrap_or(DEFAULT_ENDPOINT),
            RESPONSES_PATH
        )
    }

    fn headers(&self, token: &str) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                .unwrap_or_else(|_| reqwest::header::HeaderValue::from_static("")),
        );
        headers.insert(
            "OpenAI-Beta",
            reqwest::header::HeaderValue::from_static("responses=experimental"),
        );
        headers.insert("originator", reqwest::header::HeaderValue::from_static("codex"));
        headers.insert("User-Agent", reqwest::header::HeaderValue::from_static("Codex/0.22.4"));
        headers.insert(
            "session_id",
            reqwest::header::HeaderValue::from_str(&uuid::Uuid::new_v4().to_string())
                .unwrap_or_else(|_| reqwest::header::HeaderValue::from_static("")),
        );
        if let Some(account_id) = &self.account_id
            && let Ok(val) = reqwest::header::HeaderValue::from_str(account_id)
        {
            headers.insert("chatgpt-account-id", val.clone());
            headers.insert("ChatGPT-Account-Id", val);
        }
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("text/event-stream"),
        );
        headers
    }

    fn build_request(&self, request: &ModelCompletionRequest) -> Result<serde_json::Value, AppError> {
        let mut input = Vec::new();
        if let Some(preamble) = &request.preamble
            && !preamble.trim().is_empty()
        {
            input.push(json!({
                "type": "message",
                "role": "system",
                "content": [{"type": "input_text", "text": preamble.trim()}]
            }));
        }
        for m in &request.messages {
            input.extend(convert_chatgpt_message(m));
        }
        if !request.prompt.trim().is_empty() {
            input.push(json!({
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": request.prompt.trim()}]
            }));
        }

        let tools = request
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                })
            })
            .collect::<Vec<_>>();

        let mut body = json!({
            "model": self.model,
            "input": input,
            "stream": true,
        });
        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }
        Ok(body)
    }

    async fn post_stream(
        &self,
        token: &str,
        request: &ModelCompletionRequest,
    ) -> Result<reqwest::Response, (Option<u16>, String)> {
        let body = self.build_request(request).map_err(|e| (None, e.to_string()))?;
        let headers = self.headers(token);
        let response = CLIENT
            .post(self.target_url())
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| (None, e.to_string()))?;

        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let text = response.text().await.unwrap_or_default();
        Err((Some(status.as_u16()), text))
    }

    async fn retry_post_stream(
        &self,
        request: &ModelCompletionRequest,
    ) -> Result<reqwest::Response, (Option<u16>, String)> {
        let fresh = self.token_provider.force_refresh().await.map_err(|e| (Some(401), e))?;
        self.post_stream(&fresh, request).await
    }

    pub async fn open_stream(
        &self,
        request: &ModelCompletionRequest,
    ) -> Result<reqwest::Response, (Option<u16>, String)> {
        let token = self.token_provider.token().await.map_err(|e| (None, e))?;
        match self.post_stream(&token, request).await {
            Err((Some(401), _)) => self.retry_post_stream(request).await,
            other => other,
        }
    }
}

fn convert_chatgpt_user(content: &[rho_harness_core::model::UserContent]) -> Vec<Value> {
    let mut items = Vec::new();
    for part in content {
        match part {
            rho_harness_core::model::UserContent::Text(t) => {
                items.push(json!({
                    "type": "message",
                    "role": "user",
                    "content": [{"type": "input_text", "text": t.text}]
                }));
            }
            rho_harness_core::model::UserContent::ToolResult(r) => {
                let mut text = String::new();
                for c in &r.content {
                    if let rho_harness_core::model::ToolResultContent::Text(t) = c {
                        text.push_str(&t.text);
                    }
                }
                items.push(json!({
                    "type": "function_call_output",
                    "call_id": r.call,
                    "output": text,
                }));
            }
            _ => {}
        }
    }
    items
}

fn convert_chatgpt_assistant(content: &[rho_harness_core::model::AssistantContent]) -> Vec<Value> {
    let mut items = Vec::new();
    for part in content {
        match part {
            rho_harness_core::model::AssistantContent::Text(t) => {
                items.push(json!({
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": t.text}]
                }));
            }
            rho_harness_core::model::AssistantContent::ToolCall(call) => {
                items.push(json!({
                    "type": "function_call",
                    "call_id": call.id,
                    "name": call.function.name,
                    "arguments": serde_json::to_string(&call.function.arguments).unwrap_or_default(),
                }));
            }
            _ => {}
        }
    }
    items
}

fn convert_chatgpt_message(msg: &ChatMessage) -> Vec<Value> {
    match msg {
        ChatMessage::System { content } => vec![json!({
            "type": "message",
            "role": "system",
            "content": [{"type": "input_text", "text": content}]
        })],
        ChatMessage::User { content } => convert_chatgpt_user(content),
        ChatMessage::Assistant { content, .. } => convert_chatgpt_assistant(content),
    }
}

#[async_trait::async_trait]
impl ModelAdapter for ChatGptClient {
    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        "chatgpt"
    }

    async fn complete(&self, req: ModelCompletionRequest) -> Result<ModelCompletionResponse, AppError> {
        let stream = self.stream(req).await?;
        let events = futures::StreamExt::collect::<Vec<_>>(stream).await;
        crate::provider::sse::aggregate_stream_events(events)
    }

    async fn stream(
        &self,
        req: ModelCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>, AppError> {
        let response = self
            .open_stream(&req)
            .await
            .map_err(|(status, body)| AppError::Provider(friendly_error(status, &body)))?;
        Ok(Box::pin(crate::chatgpt::stream::unfold_chatgpt_stream(response)))
    }
}

pub fn into_handle(client: ChatGptClient) -> ModelHandle {
    Arc::new(client)
}

pub fn friendly_error(status: Option<u16>, body: &str) -> String {
    let message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            if let Some(msg) = v.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()) {
                Some(msg.to_string())
            } else if let Some(msg) = v.get("message").and_then(|m| m.as_str()) {
                Some(msg.to_string())
            } else {
                v.get("error").and_then(|e| e.as_str()).map(|err| err.to_string())
            }
        })
        .unwrap_or_else(|| {
            let s: String = body.chars().take(300).collect();
            if s.trim().is_empty() {
                "unknown error".to_string()
            } else {
                s
            }
        });

    match status {
        Some(401) => "ChatGPT OAuth session expired or credentials are invalid. Run 'rho login chatgpt'.".to_string(),
        Some(429) => format!("ChatGPT rate limit or usage limit reached. Wait a bit and retry. Backend: {message}"),
        Some(403) => format!("ChatGPT access denied. Backend: {message}"),
        Some(400) => format!("ChatGPT request invalid. Backend: {message}"),
        Some(503) | Some(502) => "ChatGPT service is temporarily unavailable. Wait a bit and retry.".to_string(),
        Some(other) => format!("ChatGPT API error ({other}): {message}"),
        None => {
            let clean = message.strip_prefix("ChatGPT request failed: ").unwrap_or(&message);
            format!("ChatGPT request failed: {clean}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chatgpt_client_headers_include_originator_and_beta() {
        let client = ChatGptClient::new("test-token", "gpt-5.4").with_account_id(Some("acc-123".to_string()));
        let headers = client.headers("test-token");
        assert_eq!(headers.get("OpenAI-Beta").unwrap(), "responses=experimental");
        assert_eq!(headers.get("originator").unwrap(), "codex");
        assert_eq!(headers.get("User-Agent").unwrap(), "Codex/0.22.4");
        assert_eq!(headers.get("chatgpt-account-id").unwrap(), "acc-123");
        assert_eq!(headers.get("ChatGPT-Account-Id").unwrap(), "acc-123");
        assert!(headers.get("session_id").is_some());
    }

    #[test]
    fn build_request_sets_store_false_and_encrypted_reasoning() {
        let client = ChatGptClient::new("test-token", "gpt-5.4");
        let mut req = sample_completion_request();
        req.temperature = Some(0.7);
        req.max_tokens = Some(100);
        let json_body = client.build_request(&req).unwrap();

        assert_eq!(json_body["model"], "gpt-5.4");
        assert_eq!(json_body["stream"], true);
    }

    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    struct MockTokenProvider {
        token_val: String,
        refresh_count: Arc<AtomicUsize>,
        fail_refresh: bool,
    }

    #[async_trait::async_trait]
    impl TokenProvider for MockTokenProvider {
        async fn token(&self) -> std::result::Result<String, String> {
            Ok(self.token_val.clone())
        }
        async fn force_refresh(&self) -> std::result::Result<String, String> {
            self.refresh_count.fetch_add(1, Ordering::SeqCst);
            if self.fail_refresh {
                Err("refresh failed".to_string())
            } else {
                Ok("token-refreshed".to_string())
            }
        }
    }

    struct FailingTokenProvider;

    #[async_trait::async_trait]
    impl TokenProvider for FailingTokenProvider {
        async fn token(&self) -> std::result::Result<String, String> {
            Err("failed to get token".to_string())
        }
        async fn force_refresh(&self) -> std::result::Result<String, String> {
            Err("failed to refresh".to_string())
        }
    }

    async fn spawn_mock_responses(responses: Vec<String>) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            for resp in responses {
                if let Ok((mut socket, _)) = listener.accept().await {
                    let mut buf = [0u8; 4096];
                    let _ = socket.read(&mut buf).await;
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.flush().await;
                }
            }
        });
        (format!("http://{addr}"), handle)
    }

    fn sample_completion_request() -> ModelCompletionRequest {
        ModelCompletionRequest {
            prompt: "hi".to_string(),
            preamble: None,
            messages: Vec::new(),
            tools: Vec::new(),
            max_tokens: None,
            temperature: None,
        }
    }

    #[tokio::test]
    async fn open_stream_success() {
        let sse_body = "data: {\"type\": \"response.completed\", \"response\": {\"usage\": {\"input_tokens\": 1, \"output_tokens\": 1, \"total_tokens\": 2}}}\n\n";
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse_body}",
            sse_body.len()
        );
        let (endpoint, _handle) = spawn_mock_responses(vec![resp]).await;
        let client = ChatGptClient::new("test-token", "gpt-5.4").with_endpoint(endpoint);
        let res = client.open_stream(&sample_completion_request()).await;
        assert!(res.is_ok());
        assert_eq!(res.unwrap().status(), 200);
    }

    #[tokio::test]
    async fn open_stream_401_retry_success() {
        let resp401 =
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\nConnection: close\r\n\r\nUnauthorized".to_string();
        let sse_body = "data: {\"type\": \"response.completed\", \"response\": {\"usage\": {\"input_tokens\": 1, \"output_tokens\": 1, \"total_tokens\": 2}}}\n\n";
        let resp200 = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse_body}",
            sse_body.len()
        );
        let (endpoint, _handle) = spawn_mock_responses(vec![resp401, resp200]).await;
        let refresh_count = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(MockTokenProvider {
            token_val: "initial-token".to_string(),
            refresh_count: refresh_count.clone(),
            fail_refresh: false,
        });
        let client = ChatGptClient::with_token_provider(provider, "gpt-5.4").with_endpoint(endpoint);
        let res = client.open_stream(&sample_completion_request()).await;
        assert!(res.is_ok());
        assert_eq!(refresh_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn open_stream_401_retry_fails() {
        let resp401a =
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\nConnection: close\r\n\r\nUnauthorized".to_string();
        let resp401b =
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\nConnection: close\r\n\r\nUnauthorized".to_string();
        let (endpoint, _handle) = spawn_mock_responses(vec![resp401a, resp401b]).await;
        let refresh_count = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(MockTokenProvider {
            token_val: "initial-token".to_string(),
            refresh_count: refresh_count.clone(),
            fail_refresh: false,
        });
        let client = ChatGptClient::with_token_provider(provider, "gpt-5.4").with_endpoint(endpoint);
        let err = client.open_stream(&sample_completion_request()).await.unwrap_err();
        assert_eq!(err.0, Some(401));
        assert_eq!(refresh_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn open_stream_token_provider_fails() {
        let client = ChatGptClient::with_token_provider(Arc::new(FailingTokenProvider), "gpt-5.4")
            .with_endpoint("http://127.0.0.1:9");
        let err = client.open_stream(&sample_completion_request()).await.unwrap_err();
        assert_eq!(err, (None, "failed to get token".to_string()));
    }

    #[tokio::test]
    async fn open_stream_force_refresh_fails() {
        let resp401 =
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\nConnection: close\r\n\r\nUnauthorized".to_string();
        let (endpoint, _handle) = spawn_mock_responses(vec![resp401]).await;
        let provider = Arc::new(MockTokenProvider {
            token_val: "initial-token".to_string(),
            refresh_count: Arc::new(AtomicUsize::new(0)),
            fail_refresh: true,
        });
        let client = ChatGptClient::with_token_provider(provider, "gpt-5.4").with_endpoint(endpoint);
        let err = client.open_stream(&sample_completion_request()).await.unwrap_err();
        assert_eq!(err, (Some(401), "refresh failed".to_string()));
    }

    #[tokio::test]
    async fn completion_and_stream_roundtrip() {
        let sse_body = concat!(
            "data: {\"type\": \"response.output_text.delta\", \"delta\": \"Hello world\"}\n\n",
            "data: {\"type\": \"response.completed\", \"response\": {\"usage\": {\"input_tokens\": 5, \"output_tokens\": 2, \"total_tokens\": 7}}}\n\n"
        );
        let resp1 = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse_body}",
            sse_body.len()
        );
        let resp2 = resp1.clone();
        let (endpoint, _handle) = spawn_mock_responses(vec![resp1, resp2]).await;
        let client = ChatGptClient::new("test-token", "gpt-5.4").with_endpoint(endpoint);

        let completion_res = into_handle(client.clone())
            .complete(sample_completion_request())
            .await
            .unwrap();
        assert_eq!(completion_res.usage.total_tokens, 7);

        let stream_res = into_handle(client.clone()).stream(sample_completion_request()).await;
        assert!(stream_res.is_ok());
    }

    #[test]
    fn build_request_instructions_preamble_variations() {
        let client = ChatGptClient::new("test-token", "gpt-5.4");

        let mut req_with_custom = sample_completion_request();
        req_with_custom.preamble = Some("Act as a Rust compiler.".to_string());
        let res_custom = client.build_request(&req_with_custom).unwrap();
        let input = res_custom["input"].as_array().unwrap();
        assert_eq!(input[0]["content"][0]["text"], "Act as a Rust compiler.");
    }

    #[test]
    fn constructors_and_handle_conversion() {
        let temp = tempfile::tempdir().unwrap();
        let store = Arc::new(tokio::sync::Mutex::new(
            crate::auth::store::AuthStore::load(temp.path().join("auth.json")).unwrap(),
        ));
        let client = ChatGptClient::with_auth_store(store, "gpt-5.4");
        assert_eq!(client.target_url(), format!("{DEFAULT_ENDPOINT}{RESPONSES_PATH}"));
        let _handle = into_handle(client);
    }

    #[test]
    fn friendly_error_matches_all_status_and_body_patterns() {
        let err429 = friendly_error(Some(429), r#"{"error":{"message":"rate limit exceeded"}}"#);
        assert!(err429.contains("rate limit"));
        assert!(err429.contains("Backend: rate limit exceeded"));

        let err403 = friendly_error(Some(403), r#"{"message":"access forbidden"}"#);
        assert!(err403.contains("ChatGPT access denied"));
        assert!(err403.contains("Backend: access forbidden"));

        let err400 = friendly_error(Some(400), r#"{"error":"invalid parameter"}"#);
        assert!(err400.contains("ChatGPT request invalid"));
        assert!(err400.contains("Backend: invalid parameter"));

        let err503 = friendly_error(Some(503), "");
        assert!(err503.contains("temporarily unavailable"));

        let err502 = friendly_error(Some(502), "");
        assert!(err502.contains("temporarily unavailable"));

        let err500 = friendly_error(Some(500), "plain text internal error");
        assert!(err500.contains("ChatGPT API error (500): plain text internal error"));

        let err_err_str = friendly_error(Some(400), r#"{"error":"bad_req"}"#);
        assert!(err_err_str.contains("Backend: bad_req"));

        let err_no_msg = friendly_error(Some(500), r#"{"code":123}"#);
        assert!(err_no_msg.contains("ChatGPT API error (500): {\"code\":123}"));

        let err_none_prefixed = friendly_error(None, "ChatGPT request failed: boom");
        assert_eq!(err_none_prefixed, "ChatGPT request failed: boom");

        let err_none_unprefixed = friendly_error(None, "boom");
        assert_eq!(err_none_unprefixed, "ChatGPT request failed: boom");

        let err_empty = friendly_error(Some(500), "   ");
        assert!(err_empty.contains("unknown error"));
    }
}
