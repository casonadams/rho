//! ChatGPT (Codex) Responses API client implementation.

use std::sync::Arc;

use crate::auth::token::{StaticTokenProvider, TokenProvider};
use crate::http::{bearer, header_value, post_json_checked};
use crate::provider::adapter::{ModelAdapter, ModelCompletionRequest, ModelEventStream};
use crate::provider::error::{HttpFailure, HttpResult};
use crate::provider::sse::open_sse_stream;
use rho_harness_core::error::AppError;
use rho_harness_core::model::ChatMessage;
use serde_json::{Value, json};

pub const DEFAULT_ENDPOINT: &str = "https://chatgpt.com/backend-api/codex";
pub const RESPONSES_PATH: &str = "/responses";

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

    fn headers(&self, token: &str) -> Result<reqwest::header::HeaderMap, HttpFailure> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::AUTHORIZATION, bearer(token)?);
        headers.insert(
            "OpenAI-Beta",
            reqwest::header::HeaderValue::from_static("responses=experimental"),
        );
        headers.insert("originator", reqwest::header::HeaderValue::from_static("codex"));
        headers.insert("User-Agent", reqwest::header::HeaderValue::from_static("Codex/0.22.4"));
        headers.insert(
            "session_id",
            header_value(&uuid::Uuid::new_v4().to_string(), "The session id")?,
        );
        if let Some(account_id) = &self.account_id {
            let val = header_value(account_id, "The account id")?;
            headers.insert("chatgpt-account-id", val.clone());
            headers.insert("ChatGPT-Account-Id", val);
        }
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("text/event-stream"),
        );
        Ok(headers)
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

    async fn post_stream(&self, token: &str, request: &ModelCompletionRequest) -> HttpResult {
        let body = self
            .build_request(request)
            .map_err(|e| HttpFailure::Local(e.to_string()))?;
        post_json_checked(&self.target_url(), self.headers(token)?, &body).await
    }

    async fn retry_post_stream(&self, request: &ModelCompletionRequest) -> HttpResult {
        let fresh = self.token_provider.force_refresh().await.map_err(HttpFailure::Local)?;
        self.post_stream(&fresh, request).await
    }

    pub async fn open_stream(&self, request: &ModelCompletionRequest) -> HttpResult {
        let token = self.token_provider.token().await.map_err(HttpFailure::Local)?;
        match self.post_stream(&token, request).await {
            Err(failure) if failure.status_code() == Some(401) => self.retry_post_stream(request).await,
            other => other,
        }
    }
}

fn input_image(img: &rho_harness_core::model::ImageContent) -> Value {
    json!({
        "type": "input_image",
        "image_url": format!("data:{};base64,{}", img.media_type_or_png(), img.data),
    })
}

fn convert_chatgpt_user(content: &[rho_harness_core::model::UserContent]) -> Vec<Value> {
    use rho_harness_core::model::{ToolResultContent, UserContent};
    let mut items = Vec::new();
    let mut tool_images = Vec::new();
    for part in content {
        match part {
            UserContent::Text(t) => {
                items.push(json!({
                    "type": "message",
                    "role": "user",
                    "content": [{"type": "input_text", "text": t.text}]
                }));
            }
            UserContent::ToolResult(r) => {
                items.push(json!({
                    "type": "function_call_output",
                    "call_id": r.call,
                    "output": r.text(),
                }));
                for item in &r.content {
                    if let ToolResultContent::Image(img) = item {
                        tool_images.push(input_image(img));
                    }
                }
            }
            UserContent::Image(img) => {
                items.push(json!({
                    "type": "message",
                    "role": "user",
                    "content": [input_image(img)]
                }));
            }
        }
    }
    if !tool_images.is_empty() {
        let mut parts = vec![json!({"type": "input_text", "text": "Images returned by the tool calls above:"})];
        parts.extend(tool_images);
        items.push(json!({"type": "message", "role": "user", "content": parts}));
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

    async fn stream(&self, req: ModelCompletionRequest) -> Result<ModelEventStream, AppError> {
        open_sse_stream(
            self.open_stream(&req).await,
            friendly_error,
            crate::chatgpt::stream::SseParser::new(),
        )
    }
}

pub fn friendly_error(status: Option<u16>, body: &str) -> String {
    crate::http::friendly_oauth_error("ChatGPT", "chatgpt", status, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chatgpt_client_headers_include_originator_and_beta() {
        let client = ChatGptClient::new("test-token", "gpt-5.4").with_account_id(Some("acc-123".to_string()));
        let headers = client.headers("test-token").unwrap();
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

    #[test]
    fn test_convert_chatgpt_user_content() {
        let content = vec![
            rho_harness_core::model::UserContent::text("hello"),
            rho_harness_core::model::UserContent::ToolResult(rho_harness_core::model::ToolResult::new(
                "call-1",
                "bash",
                "output text",
            )),
        ];
        let items = convert_chatgpt_user(&content);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["type"], "message");
        assert_eq!(items[1]["type"], "function_call_output");
        assert_eq!(items[1]["call_id"], "call-1");
    }

    use crate::test_support::{ScriptedTokenProvider, serve_raw_http};
    use std::sync::atomic::Ordering;

    fn sample_completion_request() -> ModelCompletionRequest {
        ModelCompletionRequest {
            prompt: "hi".to_string(),
            preamble: None,
            messages: Vec::new(),
            tools: Vec::new(),
            max_tokens: None,
            temperature: None,
            thinking_level: None,
        }
    }

    #[tokio::test]
    async fn open_stream_success() {
        let sse_body = "data: {\"type\": \"response.completed\", \"response\": {\"usage\": {\"input_tokens\": 1, \"output_tokens\": 1, \"total_tokens\": 2}}}\n\n";
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse_body}",
            sse_body.len()
        );
        let endpoint = serve_raw_http(vec![resp]).await;
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
        let endpoint = serve_raw_http(vec![resp401, resp200]).await;
        let provider = ScriptedTokenProvider::new(Ok("initial-token"), Ok("token-refreshed"));
        let refresh_count = provider.refresh_count.clone();
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
        let endpoint = serve_raw_http(vec![resp401a, resp401b]).await;
        let provider = ScriptedTokenProvider::new(Ok("initial-token"), Ok("token-refreshed"));
        let refresh_count = provider.refresh_count.clone();
        let client = ChatGptClient::with_token_provider(provider, "gpt-5.4").with_endpoint(endpoint);
        let err = client.open_stream(&sample_completion_request()).await.unwrap_err();
        assert_eq!(err.status_code(), Some(401));
        assert_eq!(refresh_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn open_stream_token_provider_fails() {
        let client = ChatGptClient::with_token_provider(
            ScriptedTokenProvider::new(Err("failed to get token"), Err("failed to refresh")),
            "gpt-5.4",
        )
        .with_endpoint("http://127.0.0.1:9");
        let err = client.open_stream(&sample_completion_request()).await.unwrap_err();
        assert_eq!(err, HttpFailure::Local("failed to get token".to_string()));
    }

    #[tokio::test]
    async fn open_stream_force_refresh_fails() {
        let resp401 =
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\nConnection: close\r\n\r\nUnauthorized".to_string();
        let endpoint = serve_raw_http(vec![resp401]).await;
        let provider = ScriptedTokenProvider::new(Ok("initial-token"), Err("refresh failed"));
        let client = ChatGptClient::with_token_provider(provider, "gpt-5.4").with_endpoint(endpoint);
        let err = client.open_stream(&sample_completion_request()).await.unwrap_err();
        assert_eq!(err, HttpFailure::Local("refresh failed".to_string()));
        let shown = crate::http::friendly_oauth_error("ChatGPT", "chatgpt", err.status_code(), err.body());
        assert!(
            shown.contains("refresh failed") && !shown.contains("expired"),
            "{shown}"
        );
    }

    #[test]
    fn user_and_tool_result_images_are_sent_as_input_images() {
        use rho_harness_core::model::{ToolResult, ToolResultContent, UserContent};
        let mut result = ToolResult::new("call-1", "read", "saw a picture");
        result
            .content
            .push(ToolResultContent::image("TOOLIMG", Some("image/jpeg".into())));
        let items = convert_chatgpt_user(&[
            UserContent::text("look"),
            UserContent::image("USERIMG", None),
            UserContent::ToolResult(result),
        ]);

        assert_eq!(items[1]["content"][0]["type"], "input_image");
        assert_eq!(items[1]["content"][0]["image_url"], "data:image/png;base64,USERIMG");
        assert_eq!(items[2]["type"], "function_call_output");
        assert_eq!(items[2]["output"], "saw a picture");
        let follow_up = &items[3]["content"];
        assert_eq!(items.len(), 4);
        assert_eq!(follow_up[1]["image_url"], "data:image/jpeg;base64,TOOLIMG");
    }

    #[test]
    fn tool_results_without_images_add_no_follow_up_message() {
        use rho_harness_core::model::{ToolResult, UserContent};
        let items = convert_chatgpt_user(&[UserContent::ToolResult(ToolResult::new("c", "read", "plain"))]);
        assert_eq!(items.len(), 1);
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
        let endpoint = serve_raw_http(vec![resp1, resp2]).await;
        let client = ChatGptClient::new("test-token", "gpt-5.4").with_endpoint(endpoint);

        let completion_res = client.complete(sample_completion_request()).await.unwrap();
        assert_eq!(completion_res.usage.total_tokens, 7);

        let stream_res = client.stream(sample_completion_request()).await;
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
    fn default_endpoint_is_the_codex_responses_api() {
        let client = ChatGptClient::with_token_provider(Arc::new(StaticTokenProvider::new("t")), "gpt-5.4");
        assert_eq!(client.target_url(), format!("{DEFAULT_ENDPOINT}{RESPONSES_PATH}"));
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
