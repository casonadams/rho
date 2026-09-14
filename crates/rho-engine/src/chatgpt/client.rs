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
    CompletionRequest as ResponsesRequest, ResponsesRequestParams, SystemInstructionsPlacement,
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
            .map_err(|e| (None, format!("ChatGPT request failed: {e}")))?;

        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let text = response.text().await.unwrap_or_default();
        Err((Some(status.as_u16()), text))
    }

    pub async fn open_stream(&self, request: &CompletionRequest) -> Result<reqwest::Response, (Option<u16>, String)> {
        let token = self.token_provider.token().await.map_err(|e| (None, e))?;
        match self.post_stream(&token, request).await {
            Ok(res) => Ok(res),
            Err((Some(401), _)) => {
                let fresh = self.token_provider.force_refresh().await.map_err(|e| (Some(401), e))?;
                self.post_stream(&fresh, request).await
            }
            Err(e) => Err(e),
        }
    }

    pub async fn feed_stream<F>(&self, request: &CompletionRequest, mut handler: F) -> Result<(), CompletionError>
    where
        F: FnMut(Vec<Result<RawStreamingChoice<StreamFinal>, CompletionError>>) -> Result<(), CompletionError>,
    {
        use futures::StreamExt;
        let response = self
            .open_stream(request)
            .await
            .map_err(|(status, body)| CompletionError::ProviderError(friendly_error(status, &body)))?;

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
        None => format!("ChatGPT request failed: {message}"),
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
    fn friendly_error_formats_auth_and_quota_errors() {
        let auth_err = friendly_error(Some(401), r#"{"error":{"message":"token expired"}}"#);
        assert!(auth_err.contains("Run 'rho login chatgpt'"));

        let rate_err = friendly_error(Some(429), r#"{"error":{"message":"too many requests"}}"#);
        assert!(rate_err.contains("ChatGPT rate limit or usage limit reached"));
        assert!(rate_err.contains("too many requests"));
    }
}
