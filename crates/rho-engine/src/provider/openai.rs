//! Universal OpenAI-compatible chat completions provider client.
//!
//! Supports OpenAI, DeepSeek, Groq, OpenRouter, Ollama (local and cloud),
//! xAI, Mistral, Gemini (OpenAI compat endpoint), and custom proxies.

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use rho_harness_core::error::AppError;
use rho_harness_core::model::{
    AssistantContent, ChatMessage, ImageContent, ToolCall, ToolFunction, ToolResultContent, UserContent,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::engine::metrics::StructuralUsage;
use crate::provider::adapter::{
    FinishReason, ModelAdapter, ModelCompletionRequest, ModelEventStream, ModelStreamEvent, ModelToolDefinition,
};
use crate::provider::error::{HttpFailure, classify_stream_error};
use crate::provider::json_nesting::JsonNesting;
use crate::provider::sse::{DONE_SENTINEL, SseLineDecoder, SseStreamParser, open_sse_stream};

#[derive(Clone)]
pub struct OpenAiClient {
    base_url: String,
    api_key: String,
    model: String,
    provider_name: String,
    extra_headers: HeaderMap,
}

impl std::fmt::Debug for OpenAiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiClient")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("provider_name", &self.provider_name)
            .finish_non_exhaustive()
    }
}

impl OpenAiClient {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        provider_name: impl Into<String>,
    ) -> Self {
        let mut base = base_url.into();
        while base.ends_with('/') {
            base.pop();
        }
        Self {
            base_url: base,
            api_key: api_key.into(),
            model: model.into(),
            provider_name: provider_name.into(),
            extra_headers: HeaderMap::new(),
        }
    }

    pub fn with_headers(mut self, headers: HeaderMap) -> Self {
        self.extra_headers = headers;
        self
    }

    fn endpoint_url(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }

    fn build_headers(&self) -> Result<HeaderMap, HttpFailure> {
        let mut headers = self.extra_headers.clone();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let api_key = self.api_key.trim();
        if !api_key.is_empty() {
            headers.insert(AUTHORIZATION, crate::http::bearer(api_key)?);
        }
        Ok(headers)
    }
}

fn image_part(img: &ImageContent) -> Value {
    json!({
        "type": "image_url",
        "image_url": { "url": format!("data:{};base64,{}", img.media_type_or_png(), img.data) }
    })
}

fn convert_openai_user(content: &[UserContent]) -> Vec<Value> {
    let mut messages = Vec::new();
    let mut parts = Vec::new();
    let mut tool_images = Vec::new();
    for part in content {
        match part {
            UserContent::Text(t) => parts.push(json!({ "type": "text", "text": t.text })),
            UserContent::Image(img) => parts.push(image_part(img)),
            UserContent::ToolResult(res) => {
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": res.call,
                    "content": res.text(),
                }));
                tool_images.extend(res.content.iter().filter_map(|item| match item {
                    ToolResultContent::Image(img) => Some(image_part(img)),
                    _ => None,
                }));
            }
        }
    }
    if !tool_images.is_empty() {
        parts.push(json!({ "type": "text", "text": "Images returned by the tool calls above:" }));
        parts.extend(tool_images);
    }
    if !parts.is_empty() {
        messages.push(user_message(parts));
    }
    messages
}

fn user_message(parts: Vec<Value>) -> Value {
    if parts.len() == 1 && parts[0]["type"] == "text" {
        json!({
            "role": "user",
            "content": parts[0]["text"]
        })
    } else {
        json!({
            "role": "user",
            "content": parts
        })
    }
}

fn convert_openai_assistant(content: &[AssistantContent]) -> Value {
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    for part in content {
        match part {
            AssistantContent::Text(t) => text.push_str(&t.text),
            AssistantContent::ToolCall(call) => {
                tool_calls.push(json!({
                    "id": call.id,
                    "type": "function",
                    "function": {
                        "name": call.function.name,
                        "arguments": serde_json::to_string(&call.function.arguments).unwrap_or_default()
                    }
                }));
            }
            _ => {}
        }
    }
    let mut obj = serde_json::Map::new();
    obj.insert("role".to_string(), json!("assistant"));
    if !text.is_empty() {
        obj.insert("content".to_string(), json!(text));
    }
    if !tool_calls.is_empty() {
        obj.insert("tool_calls".to_string(), json!(tool_calls));
    }
    Value::Object(obj)
}

fn convert_chat_message(msg: &ChatMessage) -> Vec<Value> {
    match msg {
        ChatMessage::System { content } => vec![json!({
            "role": "system",
            "content": content,
        })],
        ChatMessage::User { content } => convert_openai_user(content),
        ChatMessage::Assistant { content, .. } => vec![convert_openai_assistant(content)],
    }
}

fn convert_tools(tools: &[ModelToolDefinition]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters
                }
            })
        })
        .collect()
}

pub fn build_request_body(req: &ModelCompletionRequest, model: &str, stream: bool) -> Value {
    let mut messages = Vec::new();
    if let Some(preamble) = &req.preamble
        && !preamble.trim().is_empty()
    {
        messages.push(json!({ "role": "system", "content": preamble.trim() }));
    }
    messages.extend(req.messages.iter().flat_map(convert_chat_message));
    if !req.prompt.trim().is_empty() {
        messages.push(json!({ "role": "user", "content": req.prompt.trim() }));
    }

    let mut body = json!({
        "model": model,
        "messages": messages,
        "stream": stream,
    });

    if stream {
        body["stream_options"] = json!({ "include_usage": true });
    }
    if !req.tools.is_empty() {
        body["tools"] = json!(convert_tools(&req.tools));
    }
    if let Some(temp) = req.temperature {
        body["temperature"] = json!(temp);
    }
    if let Some(max) = req.max_tokens {
        body["max_tokens"] = json!(max);
    }
    body
}

#[derive(Default)]
pub struct OpenAiSseParser {
    decoder: SseLineDecoder,
    pending_tool_calls: HashMap<usize, PartialToolCall>,
    finish_seen: bool,
    done: bool,
}

#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
    nesting: JsonNesting,
}

#[derive(Deserialize)]
struct ChunkDelta {
    content: Option<String>,
    reasoning_content: Option<String>,
    reasoning: Option<String>,
    tool_calls: Option<Vec<ChunkToolCall>>,
}

#[derive(Deserialize)]
struct ChunkToolCall {
    index: Option<usize>,
    id: Option<String>,
    function: Option<ChunkFunction>,
}

#[derive(Deserialize)]
struct ChunkFunction {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct ChunkChoice {
    delta: Option<ChunkDelta>,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ChunkUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    total_tokens: Option<u64>,
    prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Deserialize)]
struct PromptTokensDetails {
    cached_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct ChatChunk {
    choices: Option<Vec<ChunkChoice>>,
    usage: Option<ChunkUsage>,
    error: Option<ChunkError>,
}

#[derive(Deserialize)]
struct ChunkError {
    code: Option<Value>,
    #[serde(rename = "type")]
    kind: Option<String>,
    message: Option<String>,
}

impl ChunkError {
    fn into_app_error(self) -> AppError {
        let code_kind = match &self.code {
            Some(Value::String(code)) => Some(code.as_str()),
            Some(Value::Number(code)) => match code.as_u64() {
                Some(429) => Some("rate_limit_exceeded"),
                Some(408 | 500 | 502 | 503 | 504 | 529) => Some("server_error"),
                _ => None,
            },
            _ => None,
        };
        let kind = code_kind.or(self.kind.as_deref());
        let message = self.message.unwrap_or_else(|| "unknown error".to_string());
        classify_stream_error(kind, message)
    }
}

fn finish_event(finish: &str) -> Result<ModelStreamEvent, AppError> {
    let reason = match finish {
        "error" => {
            let message = "The model stream ended with finish_reason \"error\"".to_string();
            return Err(classify_stream_error(None, message));
        }
        "stop" => FinishReason::Stop,
        "tool_calls" => FinishReason::ToolCalls,
        "length" => FinishReason::Length,
        "content_filter" => FinishReason::ContentFilter,
        _ => FinishReason::Other,
    };
    Ok(ModelStreamEvent::Finish(reason))
}

impl OpenAiSseParser {
    pub fn new() -> Self {
        Self::default()
    }

    fn handle_tool_chunk(
        &mut self,
        tool_chunk: ChunkToolCall,
        idx_fallback: usize,
        events: &mut Vec<Result<ModelStreamEvent, AppError>>,
    ) {
        let idx = tool_chunk.index.unwrap_or(idx_fallback);
        let entry = self.pending_tool_calls.entry(idx).or_default();
        if let Some(id) = tool_chunk.id {
            entry.id = id;
        }
        let mut fragment = String::new();
        if let Some(func) = tool_chunk.function {
            if let Some(name) = func.name {
                entry.name.push_str(&name);
            }
            if let Some(args) = func.arguments {
                entry.arguments.push_str(&args);
                fragment = args;
            }
        }
        let maybe_complete = entry.nesting.feed(&fragment) == Some(0);
        if !entry.id.is_empty() && !entry.name.is_empty() {
            events.push(Ok(ModelStreamEvent::ToolInputDelta {
                id: entry.id.clone(),
                name: entry.name.clone(),
                json: fragment,
            }));
            let parsed_args = maybe_complete
                .then(|| serde_json::from_str::<Value>(&entry.arguments).ok())
                .flatten();
            if let Some(args_val) = parsed_args {
                let call = ToolCall::new(entry.id.clone(), ToolFunction::new(entry.name.clone(), args_val));
                events.push(Ok(ModelStreamEvent::ToolCall(call)));
                self.pending_tool_calls.remove(&idx);
            }
        }
    }

    fn handle_choice(&mut self, choice: ChunkChoice, events: &mut Vec<Result<ModelStreamEvent, AppError>>) {
        if let Some(delta) = choice.delta {
            if let Some(text) = delta.content
                && !text.is_empty()
            {
                events.push(Ok(ModelStreamEvent::Text(text)));
            }
            let reasoning = delta.reasoning_content.or(delta.reasoning);
            if let Some(r) = reasoning
                && !r.is_empty()
            {
                events.push(Ok(ModelStreamEvent::Reasoning(r)));
            }
            if let Some(tools) = delta.tool_calls {
                for (i, tc) in tools.into_iter().enumerate() {
                    self.handle_tool_chunk(tc, i, events);
                }
            }
        }
        if let Some(finish) = choice.finish_reason {
            self.flush_pending_tools(events);
            let event = finish_event(&finish);
            self.finish_seen |= event.is_ok();
            self.done |= event.is_err();
            events.push(event);
        }
    }

    fn flush_pending_tools(&mut self, events: &mut Vec<Result<ModelStreamEvent, AppError>>) {
        for (_, entry) in self.pending_tool_calls.drain() {
            if !entry.id.is_empty() && !entry.name.is_empty() {
                let args_val = crate::provider::adapter::parse_tool_arguments(&entry.arguments);
                let call = ToolCall::new(entry.id, ToolFunction::new(entry.name, args_val));
                events.push(Ok(ModelStreamEvent::ToolCall(call)));
            }
        }
    }

    fn handle_usage(&self, usage: ChunkUsage, events: &mut Vec<Result<ModelStreamEvent, AppError>>) {
        let cached = usage.prompt_tokens_details.and_then(|d| d.cached_tokens);
        let structural = StructuralUsage::from_cached_prompt(
            usage.prompt_tokens,
            usage.completion_tokens,
            usage.total_tokens,
            cached,
            None,
        );
        events.push(Ok(ModelStreamEvent::Usage(structural)));
    }
}

impl SseStreamParser for OpenAiSseParser {
    fn feed(&mut self, bytes: &[u8]) -> Vec<Result<ModelStreamEvent, AppError>> {
        let lines = self.decoder.decode_payloads(bytes);
        let mut events: Vec<_> = self.decoder.take_overflow().map(Err).into_iter().collect();

        for line in lines {
            if line == DONE_SENTINEL {
                self.done = true;
                break;
            }
            let Ok(chunk) = serde_json::from_str::<ChatChunk>(&line) else {
                continue;
            };
            if let Some(error) = chunk.error {
                self.done = true;
                events.push(Err(error.into_app_error()));
                continue;
            }
            if let Some(choices) = chunk.choices {
                for choice in choices {
                    self.handle_choice(choice, &mut events);
                }
            }
            if let Some(usage) = chunk.usage {
                self.handle_usage(usage, &mut events);
            }
        }
        events
    }

    fn is_done(&self) -> bool {
        self.done
    }

    fn finish(&mut self) -> Vec<Result<ModelStreamEvent, AppError>> {
        if self.done || self.finish_seen {
            return Vec::new();
        }
        vec![Err(AppError::Network(
            "OpenAI-compatible stream closed before a finish_reason".to_string(),
        ))]
    }
}

#[async_trait::async_trait]
impl ModelAdapter for OpenAiClient {
    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        &self.provider_name
    }

    async fn stream(&self, req: ModelCompletionRequest) -> Result<ModelEventStream, AppError> {
        let body = build_request_body(&req, &self.model, true);
        let opened = match self.build_headers() {
            Ok(headers) => crate::http::post_json_checked(&self.endpoint_url(), headers, &body).await,
            Err(failure) => Err(failure),
        };
        let opened = opened.map_err(|failure| name_provider(failure, &self.provider_name));
        open_sse_stream(opened, |_, body| body.to_string(), OpenAiSseParser::new())
    }
}

fn name_provider(failure: HttpFailure, provider: &str) -> HttpFailure {
    match failure {
        HttpFailure::Status {
            status,
            body,
            retry_after,
        } => HttpFailure::Status {
            status,
            body: format!("HTTP {status} from {provider}: {body}"),
            retry_after,
        },
        HttpFailure::Transport(error) => HttpFailure::Transport(format!("Failed to connect to {provider}: {error}")),
        local @ HttpFailure::Local(_) => local,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_harness_core::model::{
        AssistantContent, ChatMessage, ImageContent, TextContent, ToolCall, ToolFunction, ToolResult,
        ToolResultContent, UserContent,
    };
    use test_case::test_case;

    #[test]
    fn test_convert_openai_user_and_assistant() {
        let single_user = ChatMessage::user("plain prompt");
        let [single_val] = convert_chat_message(&single_user).try_into().unwrap();
        assert_eq!(single_val["role"], "user");
        assert_eq!(single_val["content"], "plain prompt");

        let system_msg = ChatMessage::system("system instruction");
        let [sys_val] = convert_chat_message(&system_msg).try_into().unwrap();
        assert_eq!(sys_val["role"], "system");

        let asst_msg = ChatMessage::Assistant {
            id: None,
            content: vec![
                AssistantContent::Text(TextContent::new("I will help")),
                AssistantContent::ToolCall(ToolCall::new(
                    "call-9",
                    ToolFunction::new("read", json!({"path": "file.rs"})),
                )),
            ],
        };
        let [asst_val] = convert_chat_message(&asst_msg).try_into().unwrap();
        assert_eq!(asst_val["role"], "assistant");
        assert_eq!(asst_val["content"], "I will help");
        assert_eq!(asst_val["tool_calls"][0]["id"], "call-9");
    }

    #[test]
    fn user_turn_keeps_every_tool_result_then_text_and_images() {
        let image = |data: &str, mime: &str| ImageContent {
            data: data.to_string(),
            media_type: Some(mime.to_string()),
        };
        let mut first = ToolResult::new("call-1", "screenshot", "shot taken");
        first
            .content
            .push(ToolResultContent::Image(image("toolpng", "image/png")));
        let req = ModelCompletionRequest {
            messages: vec![ChatMessage::User {
                content: vec![
                    UserContent::ToolResult(first),
                    UserContent::Text(TextContent::new("Hello")),
                    UserContent::ToolResult(ToolResult::new("call-2", "bash", "output")),
                    UserContent::Image(image("userjpg", "image/jpeg")),
                ],
            }]
            .into(),
            ..Default::default()
        };

        let body = build_request_body(&req, "m", false);

        assert_eq!(
            body["messages"],
            json!([
                {"role": "tool", "tool_call_id": "call-1", "content": "shot taken"},
                {"role": "tool", "tool_call_id": "call-2", "content": "output"},
                {"role": "user", "content": [
                    {"type": "text", "text": "Hello"},
                    {"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,userjpg"}},
                    {"type": "text", "text": "Images returned by the tool calls above:"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,toolpng"}},
                ]},
            ])
        );
    }

    #[test_case(r#"{"error":{"code":429,"message":"Rate limit exceeded"}}"#, "rate_limited"; "rate limit code")]
    #[test_case(
        r#"{"error":{"code":"insufficient_quota","message":"out of credit"}}"#,
        "provider";
        "insufficient quota"
    )]
    #[test_case(r#"{"error":{"code":502,"message":"Provider overloaded"}}"#, "unavailable"; "bad gateway code")]
    #[test_case(
        r#"{"error":{"code":"server_is_overloaded","message":"busy"}}"#,
        "unavailable";
        "overloaded code"
    )]
    #[test_case(r#"{"choices":[{"delta":{},"finish_reason":"error"}]}"#, "provider"; "error finish reason")]
    #[test_case(
        r#"{"error":{"code":"server_error","message":"upstream died"},"choices":[{"delta":{"content":""},"finish_reason":"error"}]}"#,
        "unavailable";
        "server error with error finish"
    )]
    fn error_chunks_and_error_finish_surface_as_typed_errors(line: &str, expected: &str) {
        let events = OpenAiSseParser::new().feed(format!("data: {line}\n\n").as_bytes());
        let kind = match events.as_slice() {
            [Err(AppError::RateLimited { .. })] => "rate_limited",
            [Err(AppError::ProviderUnavailable { .. })] => "unavailable",
            [Err(AppError::Provider(_))] => "provider",
            other => panic!("{other:?}"),
        };
        assert_eq!(kind, expected);
    }

    #[test]
    fn unsendable_api_key_fails_locally_instead_of_dropping_authorization() {
        let client = OpenAiClient::new("http://unused", "sk-secret\nvalue", "m", "openai");
        let failure = client.build_headers().unwrap_err();
        assert!(matches!(failure, HttpFailure::Local(_)), "{failure:?}");
        assert!(!failure.body().contains("sk-secret"), "{failure:?}");

        let headers = OpenAiClient::new("http://unused", " abc ", "m", "openai")
            .build_headers()
            .unwrap();
        assert_eq!(headers[AUTHORIZATION], "Bearer abc");
        let anonymous = OpenAiClient::new("http://unused", "", "m", "openai");
        assert!(!anonymous.build_headers().unwrap().contains_key(AUTHORIZATION));
    }

    #[tokio::test]
    async fn stream_with_unsendable_key_fails_before_connecting() {
        let client = OpenAiClient::new("http://unused.invalid", "sk-secret\nvalue", "m", "openai");
        let Err(error) = client.stream(ModelCompletionRequest::default()).await else {
            panic!("stream opened with an unsendable key");
        };
        assert!(
            matches!(&error, AppError::Provider(m) if m.contains("cannot be sent")),
            "{error}"
        );
    }

    #[test_case(HttpFailure::status(401, "bad key"), "HTTP 401 from groq: bad key"; "http status")]
    #[test_case(HttpFailure::Transport("refused".into()), "Failed to connect to groq: refused"; "transport")]
    #[test_case(HttpFailure::Local("no key".into()), "no key"; "local")]
    fn open_failures_name_the_provider(failure: HttpFailure, expected: &str) {
        assert_eq!(name_provider(failure, "groq").body(), expected);
    }

    #[test]
    fn test_openai_sse_parser_stream() {
        let mut parser = OpenAiSseParser::new();
        let chunks = [
            "data: {\"choices\": [{\"delta\": {\"content\": \"Hello \"}}]}\n\n",
            "data: {\"choices\": [{\"delta\": {\"content\": \"world\"}}]}\n\n",
            "data: {\"choices\": [{\"delta\": {\"tool_calls\": [{\"index\": 0, \"id\": \"call_1\", \"function\": {\"name\": \"bash\", \"arguments\": \"{\\\"cmd\\\": \"}}]}, \"finish_reason\": null}]}\n\n",
            "data: {\"choices\": [{\"delta\": {\"tool_calls\": [{\"index\": 0, \"function\": {\"arguments\": \"\\\"ls\\\"}\"}}]}, \"finish_reason\": \"tool_calls\"}], \"usage\": {\"prompt_tokens\": 10, \"completion_tokens\": 5, \"total_tokens\": 15, \"prompt_tokens_details\": {\"cached_tokens\": 2}}}\n\n",
            "data: [DONE]\n\n",
        ];

        let mut events = Vec::new();
        for chunk in chunks {
            events.extend(parser.feed(chunk.as_bytes()));
        }

        let input_delta = |event: &Result<ModelStreamEvent, AppError>, fragment: &str| {
            matches!(event, Ok(ModelStreamEvent::ToolInputDelta { id, name, json })
                if id == "call_1" && name == "bash" && json == fragment)
        };
        assert_eq!(events.len(), 7);
        assert!(matches!(&events[0], Ok(ModelStreamEvent::Text(t)) if t == "Hello "));
        assert!(matches!(&events[1], Ok(ModelStreamEvent::Text(t)) if t == "world"));
        assert!(input_delta(&events[2], "{\"cmd\": "));
        assert!(input_delta(&events[3], "\"ls\"}"));
        assert!(
            matches!(&events[4], Ok(ModelStreamEvent::ToolCall(call)) if call.id == "call_1" && call.function.name == "bash")
        );
        assert!(matches!(
            &events[5],
            Ok(ModelStreamEvent::Finish(FinishReason::ToolCalls))
        ));
        assert!(
            matches!(&events[6], Ok(ModelStreamEvent::Usage(u)) if u.total_tokens == 15 && u.input_tokens == 8 && u.cached_input_tokens == Some(2))
        );
    }

    #[test]
    fn chunked_tool_arguments_emit_one_call_when_the_object_closes() {
        let args = r#"{"path":"a.rs","content":"fn main() { let s = \"}\"; }\n"}"#;
        let mut parser = OpenAiSseParser::new();
        let mut calls = Vec::new();
        for (i, piece) in args.as_bytes().chunks(3).enumerate() {
            let fragment = std::str::from_utf8(piece).unwrap();
            let mut tool = json!({"index": 0, "function": {"arguments": fragment}});
            if i == 0 {
                tool["id"] = json!("call_1");
                tool["function"]["name"] = json!("write");
            }
            let line = json!({"choices": [{"delta": {"tool_calls": [tool]}}]});
            for event in parser.feed(format!("data: {line}\n\n").as_bytes()) {
                if let Ok(ModelStreamEvent::ToolCall(call)) = event {
                    calls.push((i, call));
                }
            }
        }
        let last = args.len().div_ceil(3) - 1;
        let [(at, call)] = calls.try_into().unwrap();
        assert_eq!(at, last);
        assert_eq!(call.function.arguments, serde_json::from_str::<Value>(args).unwrap());
    }

    async fn collect_body(payloads: &[&str]) -> Vec<Result<ModelStreamEvent, AppError>> {
        crate::provider::sse::collect_sse_payloads(payloads, OpenAiSseParser::new()).await
    }

    #[tokio::test]
    async fn stream_closed_before_finish_reason_is_a_network_error() {
        let events = collect_body(&[r#"{"choices":[{"delta":{"content":"cut off"}}]}"#]).await;
        assert!(matches!(&events[0], Ok(ModelStreamEvent::Text(t)) if t == "cut off"));
        assert!(matches!(events.last(), Some(Err(AppError::Network(_)))), "{events:?}");
    }

    #[tokio::test]
    async fn stream_closed_after_finish_reason_without_done_is_complete() {
        let events = collect_body(&[r#"{"choices":[{"delta":{"content":"hi"},"finish_reason":"stop"}]}"#]).await;
        assert!(events.iter().all(Result::is_ok), "{events:?}");
        assert!(matches!(
            events.last(),
            Some(Ok(ModelStreamEvent::Finish(FinishReason::Stop)))
        ));
    }

    #[tokio::test]
    async fn usage_after_finish_reason_is_delivered_and_done_ends_the_stream() {
        let events = collect_body(&[
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}}"#,
            "[DONE]",
            r#"{"choices":[{"delta":{"content":"after done"}}]}"#,
        ])
        .await;
        assert_eq!(events.len(), 2, "{events:?}");
        assert!(matches!(&events[1], Ok(ModelStreamEvent::Usage(u)) if u.total_tokens == 5));
    }

    #[test]
    fn test_openai_sse_parser_finish_reasons() {
        let mut parser = OpenAiSseParser::new();
        let line = "data: {\"choices\": [{\"delta\": {}, \"finish_reason\": \"stop\"}]}\n\n";
        let events = parser.feed(line.as_bytes());
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], Ok(ModelStreamEvent::Finish(FinishReason::Stop))));

        let line_len = "data: {\"choices\": [{\"delta\": {}, \"finish_reason\": \"length\"}]}\n\n";
        let events_len = parser.feed(line_len.as_bytes());
        assert_eq!(events_len.len(), 1);
        assert!(matches!(
            &events_len[0],
            Ok(ModelStreamEvent::Finish(FinishReason::Length))
        ));

        let line_content = "data: {\"choices\": [{\"delta\": {}, \"finish_reason\": \"content_filter\"}]}\n\n";
        let events_content = parser.feed(line_content.as_bytes());
        assert_eq!(events_content.len(), 1);
        assert!(matches!(
            &events_content[0],
            Ok(ModelStreamEvent::Finish(FinishReason::ContentFilter))
        ));
    }
}
