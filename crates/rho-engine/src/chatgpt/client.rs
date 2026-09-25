//! ChatGPT (Codex) Responses API client implementation.

use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::sync::Mutex;

use super::stream::SseParser;
use crate::auth::store::AuthStore;
use crate::auth::token::{AuthStoreTokenProvider, StaticTokenProvider, TokenProvider};
use crate::provider::sse::{aggregate_stream_events, unfold_sse_stream};
use rig::completion::{CompletionError, CompletionModel, CompletionRequest, CompletionResponse};
use rig::providers::openai::responses_api::{
    CompletionRequest as ResponsesRequest, Include, ResponsesRequestParams, SystemInstructionsPlacement,
};
use rig::streaming::{RawStreamingChoice, StreamFinal, StreamingCompletionResponse};

pub const DEFAULT_ENDPOINT: &str = "https://chatgpt.com/backend-api/codex";
pub const RESPONSES_PATH: &str = "/responses";
pub const PROVIDER_NAME: &str = "chatgpt";
const DEFAULT_INSTRUCTIONS: &str = "You are ChatGPT, a helpful AI assistant.";

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

    fn build_request(&self, request: CompletionRequest) -> Result<ResponsesRequest, CompletionError> {
        let mut req = ResponsesRequest::try_from(ResponsesRequestParams {
            model: self.model.clone(),
            request,
            system_instructions_placement: SystemInstructionsPlacement::AllInstructions,
        })?;

        let instructions = match req.instructions.take() {
            Some(existing) if !existing.contains(DEFAULT_INSTRUCTIONS) => {
                format!("{DEFAULT_INSTRUCTIONS}\n\n{existing}")
            }
            Some(existing) => existing,
            None => DEFAULT_INSTRUCTIONS.to_string(),
        };
        req.instructions = Some(instructions);
        req.temperature = None;
        req.max_output_tokens = None;
        req.stream = Some(true);

        let include = req.additional_parameters.include.get_or_insert_with(Vec::new);
        if !include
            .iter()
            .any(|item| matches!(item, Include::ReasoningEncryptedContent))
        {
            include.push(Include::ReasoningEncryptedContent);
        }

        req.additional_parameters.background = None;
        req.additional_parameters.metadata.clear();
        req.additional_parameters.parallel_tool_calls = None;
        req.additional_parameters.service_tier = None;
        req.additional_parameters.store = Some(false);
        req.additional_parameters.text = None;
        req.additional_parameters.top_p = None;
        req.additional_parameters.user = None;

        Ok(req)
    }

    async fn post_stream(
        &self,
        token: &str,
        request: &CompletionRequest,
    ) -> Result<reqwest::Response, (Option<u16>, String)> {
        let body = self.build_request(request.clone()).map_err(|e| (None, e.to_string()))?;
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

    async fn retry_post_stream(&self, request: &CompletionRequest) -> Result<reqwest::Response, (Option<u16>, String)> {
        let fresh = self.token_provider.force_refresh().await.map_err(|e| (Some(401), e))?;
        self.post_stream(&fresh, request).await
    }

    pub async fn open_stream(&self, request: &CompletionRequest) -> Result<reqwest::Response, (Option<u16>, String)> {
        let token = self.token_provider.token().await.map_err(|e| (None, e))?;
        match self.post_stream(&token, request).await {
            Err((Some(401), _)) => self.retry_post_stream(request).await,
            other => other,
        }
    }

    pub async fn feed_stream<F>(&self, request: &CompletionRequest, handler: F) -> Result<(), CompletionError>
    where
        F: FnMut(Vec<Result<RawStreamingChoice<StreamFinal>, CompletionError>>) -> Result<(), CompletionError>,
    {
        let response = self
            .open_stream(request)
            .await
            .map_err(|(status, body)| CompletionError::ProviderError(friendly_error(status, &body)))?;
        feed_response_stream(response, handler).await
    }
}

async fn feed_response_stream<F>(response: reqwest::Response, mut handler: F) -> Result<(), CompletionError>
where
    F: FnMut(Vec<Result<RawStreamingChoice<StreamFinal>, CompletionError>>) -> Result<(), CompletionError>,
{
    use futures::StreamExt;
    let mut parser = SseParser::new();
    let mut byte_stream = response.bytes_stream();
    while let Some(chunk) = byte_stream.next().await {
        let bytes = chunk.map_err(|e| CompletionError::ProviderError(e.to_string()))?;
        let events = parser.feed(bytes.as_ref());
        if !events.is_empty() {
            handler(events)?;
        }
    }
    Ok(())
}

pub fn into_handle(client: ChatGptClient) -> rig::agent::ModelHandle {
    rig::agent::ModelHandle::named(PROVIDER_NAME, client)
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

impl CompletionModel for ChatGptClient {
    async fn completion(&self, request: CompletionRequest) -> Result<CompletionResponse, CompletionError> {
        let mut events: Vec<Result<RawStreamingChoice<StreamFinal>, CompletionError>> = Vec::new();
        self.feed_stream(&request, |batch| {
            events.extend(batch);
            Ok(())
        })
        .await?;
        aggregate_stream_events(events, PROVIDER_NAME)
    }

    async fn stream(&self, request: CompletionRequest) -> Result<StreamingCompletionResponse, CompletionError> {
        let response = self
            .open_stream(&request)
            .await
            .map_err(|(status, body)| CompletionError::ProviderError(friendly_error(status, &body)))?;

        let stream = unfold_sse_stream(response, SseParser::new());
        Ok(StreamingCompletionResponse::stream(PROVIDER_NAME, stream))
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
        let req = client
            .build_request(CompletionRequest {
                model: None,
                output_schema: None,
                record_telemetry_content: false,
                documents: Vec::new(),
                tools: Vec::new(),
                temperature: Some(0.7),
                max_tokens: Some(100),
                tool_choice: None,
                additional_params: None,
                chat_history: vec![rig::message::Message::user("hi")],
                preamble: None,
            })
            .unwrap();

        assert_eq!(req.additional_parameters.store, Some(false));
        assert!(req.temperature.is_none());
        assert!(req.max_output_tokens.is_none());
        let includes = req.additional_parameters.include.unwrap();
        assert!(includes.iter().any(|i| matches!(i, Include::ReasoningEncryptedContent)));
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

    fn sample_completion_request() -> CompletionRequest {
        CompletionRequest {
            model: None,
            output_schema: None,
            record_telemetry_content: false,
            documents: Vec::new(),
            tools: Vec::new(),
            temperature: None,
            max_tokens: None,
            tool_choice: None,
            additional_params: None,
            chat_history: vec![rig::message::Message::user("hi")],
            preamble: None,
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

        let completion_res = client.completion(sample_completion_request()).await.unwrap();
        assert_eq!(completion_res.usage.total_tokens, 7);

        let stream_res = client.stream(sample_completion_request()).await;
        assert!(stream_res.is_ok());
    }

    #[tokio::test]
    async fn feed_stream_error_on_non_success_response() {
        let resp500 =
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 16\r\nConnection: close\r\n\r\nServer exploded!"
                .to_string();
        let (endpoint, _handle) = spawn_mock_responses(vec![resp500]).await;
        let client = ChatGptClient::new("test-token", "gpt-5.4").with_endpoint(endpoint);
        let err = client
            .feed_stream(&sample_completion_request(), |_| Ok(()))
            .await
            .unwrap_err();
        assert!(matches!(err, CompletionError::ProviderError(_)));
    }

    #[tokio::test]
    async fn feed_stream_handler_error_stops_stream() {
        let sse_body = "data: {\"type\": \"response.output_text.delta\", \"delta\": \"Hello\"}\n\n";
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse_body}",
            sse_body.len()
        );
        let (endpoint, _handle) = spawn_mock_responses(vec![resp]).await;
        let client = ChatGptClient::new("test-token", "gpt-5.4").with_endpoint(endpoint);
        let err = client
            .feed_stream(&sample_completion_request(), |_| {
                Err(CompletionError::ResponseError("handler aborted".to_string()))
            })
            .await
            .unwrap_err();
        assert!(matches!(err, CompletionError::ResponseError(_)));
    }

    #[test]
    fn build_request_instructions_preamble_variations() {
        let client = ChatGptClient::new("test-token", "gpt-5.4");

        let mut req_with_custom = sample_completion_request();
        req_with_custom.preamble = Some("Act as a Rust compiler.".to_string());
        let res_custom = client.build_request(req_with_custom).unwrap();
        assert_eq!(
            res_custom.instructions.unwrap(),
            format!("{DEFAULT_INSTRUCTIONS}\n\nAct as a Rust compiler.")
        );

        let mut req_with_default = sample_completion_request();
        req_with_default.preamble = Some(format!("{DEFAULT_INSTRUCTIONS} Be concise."));
        let res_default = client.build_request(req_with_default).unwrap();
        assert_eq!(
            res_default.instructions.unwrap(),
            format!("{DEFAULT_INSTRUCTIONS} Be concise.")
        );
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
