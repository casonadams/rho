//! Native Google Generative Language API (`v1beta`) `ModelAdapter` client.

use super::request::build_request_body;
use super::stream::GeminiSseParser;
use crate::http::{header_value, post_json_checked};
use crate::provider::adapter::{ModelAdapter, ModelCompletionRequest, ModelEventStream};
use crate::provider::error::{HttpFailure, HttpResult};
use crate::provider::sse::open_sse_stream;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue};
use rho_harness_core::error::AppError;

pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

#[derive(Clone)]
pub struct GeminiClient {
    base_url: String,
    api_key: String,
    model: String,
    thinking_level: Option<String>,
}

impl std::fmt::Debug for GeminiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeminiClient")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("thinking_level", &self.thinking_level)
            .finish_non_exhaustive()
    }
}

impl GeminiClient {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: api_key.into(),
            model: model.into(),
            thinking_level: None,
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        let mut base = base_url.into();
        while base.ends_with('/') {
            base.pop();
        }
        self.base_url = base;
        self
    }

    pub fn with_thinking_level(mut self, level: Option<String>) -> Self {
        self.thinking_level = level;
        self
    }

    fn stream_url(&self) -> String {
        let model = self.model.strip_prefix("models/").unwrap_or(&self.model);
        format!("{}/models/{model}:streamGenerateContent?alt=sse", self.base_url)
    }

    fn headers(&self) -> Result<HeaderMap, HttpFailure> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
        headers.insert(
            "x-goog-api-key",
            header_value(self.api_key.trim(), "The Gemini API key")?,
        );
        Ok(headers)
    }

    async fn open_stream(&self, request: &ModelCompletionRequest) -> HttpResult {
        let body = build_request_body(
            &self.model,
            request.effective_thinking(self.thinking_level.as_deref()),
            request,
        );
        post_json_checked(&self.stream_url(), self.headers()?, &body).await
    }
}

fn parse_google_error(body: &str) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return body.chars().take(300).collect();
    };
    let Some(err) = v.get("error") else {
        return body.chars().take(300).collect();
    };
    err.get("message")
        .and_then(|m| m.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| body.chars().take(300).collect())
}

fn friendly_error(status: Option<u16>, body: &str) -> String {
    let message = parse_google_error(body);
    match status {
        Some(401) => "Gemini API key rejected (401). Check GEMINI_API_KEY or run 'rho login gemini'.".to_string(),
        Some(403) => format!("Gemini access denied (403): {message}"),
        Some(404) => format!("Gemini model not found (404): {message}"),
        Some(429) => format!("Gemini rate limit or quota exceeded (429): {message}"),
        Some(code) => format!("Gemini API error ({code}): {message}"),
        None => format!("Gemini request failed: {message}"),
    }
}

#[async_trait::async_trait]
impl ModelAdapter for GeminiClient {
    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        "gemini"
    }

    async fn stream(&self, req: ModelCompletionRequest) -> Result<ModelEventStream, AppError> {
        open_sse_stream(self.open_stream(&req).await, friendly_error, GeminiSseParser::new())
    }

    fn cache_settings(&self) -> Option<&str> {
        self.thinking_level.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::adapter::ModelStreamEvent;
    use futures::StreamExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn debug_redacts_api_key() {
        let client = GeminiClient::new("AIza-super-secret", "gemini-4-neon");
        let debug = format!("{client:?}");
        assert!(!debug.contains("AIza-super-secret"), "{debug}");
        assert!(debug.contains("gemini-4-neon"));
    }

    #[tokio::test]
    async fn streams_native_gemini_sse_over_loopback() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = sock.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let sse = concat!(
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"thinking\",\"thought\":true,\"thoughtSignature\":\"YWJjZA==\"}]}}]}\n\n",
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hello\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":10,\"candidatesTokenCount\":5,\"thoughtsTokenCount\":3,\"totalTokenCount\":18}}\n\n",
            );
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                sse.len(),
                sse
            );
            sock.write_all(resp.as_bytes()).await.unwrap();
            req
        });

        let client = GeminiClient::new("test-key", "gemini-4-neon")
            .with_base_url(format!("http://{addr}/v1beta"))
            .with_thinking_level(Some("high".to_string()));
        let mut stream = client
            .stream(ModelCompletionRequest {
                prompt: "hi".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();

        let mut texts = Vec::new();
        let mut saw_reasoning_block = false;
        while let Some(item) = stream.next().await {
            match item.unwrap() {
                ModelStreamEvent::Text(t) => texts.push(t),
                ModelStreamEvent::ReasoningBlock(b) => {
                    assert_eq!(b.first_text_and_signature().1, Some("YWJjZA=="));
                    saw_reasoning_block = true;
                }
                _ => {}
            }
        }
        assert_eq!(texts, vec!["hello"]);
        assert!(saw_reasoning_block);

        let raw_req = server.await.unwrap();
        assert!(raw_req.contains("POST /v1beta/models/gemini-4-neon:streamGenerateContent?alt=sse"));
        assert!(raw_req.to_ascii_lowercase().contains("x-goog-api-key: test-key"));
        assert!(raw_req.contains("\"thinkingLevel\":\"HIGH\""));
        assert!(raw_req.contains("\"includeThoughts\":true"));
    }

    #[tokio::test]
    async fn request_thinking_level_overrides_client_and_maps_auto_to_medium() {
        use rho_harness_core::thinking::ThinkingLevel;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut reqs = Vec::new();
            for _ in 0..2 {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap();
                reqs.push(String::from_utf8_lossy(&buf[..n]).to_string());
                let sse = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"ok\"}]},\"finishReason\":\"STOP\"}]}\n\n";
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    sse.len(),
                    sse
                );
                sock.write_all(resp.as_bytes()).await.unwrap();
            }
            reqs
        });

        let client = GeminiClient::new("test-key", "gemini-4-neon")
            .with_base_url(format!("http://{addr}/v1beta"))
            .with_thinking_level(Some("auto".to_string()));

        let mut s1 = client
            .stream(ModelCompletionRequest {
                prompt: "hi".to_string(),
                thinking_level: Some(ThinkingLevel::High),
                ..Default::default()
            })
            .await
            .unwrap();
        while s1.next().await.is_some() {}

        let mut s2 = client
            .stream(ModelCompletionRequest {
                prompt: "hi".to_string(),
                thinking_level: Some(ThinkingLevel::Auto),
                ..Default::default()
            })
            .await
            .unwrap();
        while s2.next().await.is_some() {}

        let raw = server.await.unwrap();
        assert!(raw[0].contains("\"thinkingLevel\":\"HIGH\""), "{}", raw[0]);
        assert!(raw[1].contains("\"thinkingLevel\":\"MEDIUM\""), "{}", raw[1]);
    }
}
