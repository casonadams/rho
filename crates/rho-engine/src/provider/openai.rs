//! Universal OpenAI-compatible chat completions provider client.
//!
//! Supports OpenAI, DeepSeek, Groq, OpenRouter, Ollama (local and cloud),
//! xAI, Mistral, Gemini (OpenAI compat endpoint), and custom proxies.

use futures::Stream;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use rho_harness_core::error::AppError;
use rho_harness_core::model::{AssistantContent, ChatMessage, ToolCall, ToolFunction, ToolResultContent, UserContent};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::LazyLock;

use crate::engine::metrics::StructuralUsage;
use crate::provider::adapter::{
    FinishReason, ModelAdapter, ModelCompletionRequest, ModelCompletionResponse, ModelStreamEvent, ModelToolDefinition,
};
use crate::provider::sse::{SseLineDecoder, SseStreamParser, aggregate_stream_events, unfold_sse_stream};

static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("Failed to build HTTP client for OpenAI-compatible provider")
});

#[derive(Clone, Debug)]
pub struct OpenAiClient {
    base_url: String,
    api_key: String,
    model: String,
    provider_name: String,
    extra_headers: HeaderMap,
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

    fn build_headers(&self) -> HeaderMap {
        let mut headers = self.extra_headers.clone();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if !self.api_key.trim().is_empty()
            && let Ok(auth_val) = HeaderValue::from_str(&format!("Bearer {}", self.api_key.trim()))
        {
            headers.insert(AUTHORIZATION, auth_val);
        }
        headers
    }
}

fn convert_tool_result_content(content: &[ToolResultContent]) -> String {
    let mut text = String::new();
    for c in content {
        if let ToolResultContent::Text(t) = c {
            text.push_str(&t.text);
        }
    }
    text
}

fn convert_openai_user(content: &[UserContent]) -> Option<Value> {
    let mut parts = Vec::new();
    for part in content {
        match part {
            UserContent::Text(t) => {
                parts.push(json!({ "type": "text", "text": t.text }));
            }
            UserContent::Image(img) => {
                let mime = img.media_type.as_deref().unwrap_or("image/png");
                let data_url = format!("data:{mime};base64,{}", img.data);
                parts.push(json!({
                    "type": "image_url",
                    "image_url": { "url": data_url }
                }));
            }
            UserContent::ToolResult(res) => {
                let text = convert_tool_result_content(&res.content);
                return Some(json!({
                    "role": "tool",
                    "tool_call_id": res.call,
                    "content": text,
                }));
            }
        }
    }
    if parts.len() == 1 && parts[0]["type"] == "text" {
        Some(json!({
            "role": "user",
            "content": parts[0]["text"]
        }))
    } else {
        Some(json!({
            "role": "user",
            "content": parts
        }))
    }
}

fn convert_openai_assistant(content: &[AssistantContent]) -> Option<Value> {
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
    Some(Value::Object(obj))
}

fn convert_chat_message(msg: &ChatMessage) -> Option<Value> {
    match msg {
        ChatMessage::System { content } => Some(json!({
            "role": "system",
            "content": content,
        })),
        ChatMessage::User { content } => convert_openai_user(content),
        ChatMessage::Assistant { content, .. } => convert_openai_assistant(content),
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
    for m in &req.messages {
        if let Some(v) = convert_chat_message(m) {
            messages.push(v);
        }
    }
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
}

#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
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
        if let Some(func) = tool_chunk.function {
            if let Some(name) = func.name {
                entry.name.push_str(&name);
            }
            if let Some(args) = func.arguments {
                entry.arguments.push_str(&args);
            }
        }
        if !entry.id.is_empty() && !entry.name.is_empty() {
            let parsed_args = serde_json::from_str::<Value>(&entry.arguments).ok();
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
            let reason = match finish.as_str() {
                "stop" => FinishReason::Stop,
                "tool_calls" => FinishReason::ToolCalls,
                "length" => FinishReason::Length,
                "content_filter" => FinishReason::ContentFilter,
                _ => FinishReason::Other,
            };
            events.push(Ok(ModelStreamEvent::Finish(reason)));
        }
    }

    fn flush_pending_tools(&mut self, events: &mut Vec<Result<ModelStreamEvent, AppError>>) {
        for (_, entry) in self.pending_tool_calls.drain() {
            if !entry.id.is_empty() && !entry.name.is_empty() {
                let args_val = serde_json::from_str::<Value>(&entry.arguments)
                    .unwrap_or_else(|_| json!({ "raw": entry.arguments }));
                let call = ToolCall::new(entry.id, ToolFunction::new(entry.name, args_val));
                events.push(Ok(ModelStreamEvent::ToolCall(call)));
            }
        }
    }

    fn handle_usage(&self, usage: ChunkUsage, events: &mut Vec<Result<ModelStreamEvent, AppError>>) {
        let cached = usage.prompt_tokens_details.and_then(|d| d.cached_tokens);
        let structural = StructuralUsage {
            input_tokens: usage.prompt_tokens.unwrap_or(0),
            output_tokens: usage.completion_tokens.unwrap_or(0),
            total_tokens: usage.total_tokens.unwrap_or(0),
            cached_input_tokens: cached,
            cache_creation_input_tokens: None,
            tool_use_prompt_tokens: None,
            reasoning_tokens: None,
        };
        events.push(Ok(ModelStreamEvent::Usage(structural)));
    }
}

impl SseStreamParser for OpenAiSseParser {
    fn feed(&mut self, bytes: &[u8]) -> Vec<Result<ModelStreamEvent, AppError>> {
        let lines = self.decoder.decode_lines(bytes);
        let mut events = Vec::new();

        for line in lines {
            let Ok(chunk) = serde_json::from_str::<ChatChunk>(&line) else {
                continue;
            };
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
}

#[async_trait::async_trait]
impl ModelAdapter for OpenAiClient {
    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        &self.provider_name
    }

    async fn complete(&self, req: ModelCompletionRequest) -> Result<ModelCompletionResponse, AppError> {
        let body = build_request_body(&req, &self.model, false);
        let resp = HTTP_CLIENT
            .post(self.endpoint_url())
            .headers(self.build_headers())
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Provider(format!("Failed to connect to {}: {e}", self.provider_name)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::Provider(format!(
                "HTTP {status} from {}: {text}",
                self.provider_name
            )));
        }

        let stream = self.stream(req).await?;
        let events = futures::StreamExt::collect::<Vec<_>>(stream).await;
        aggregate_stream_events(events)
    }

    async fn stream(
        &self,
        req: ModelCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>, AppError> {
        let body = build_request_body(&req, &self.model, true);
        let resp = HTTP_CLIENT
            .post(self.endpoint_url())
            .headers(self.build_headers())
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Provider(format!("Failed to connect to {}: {e}", self.provider_name)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::Provider(format!(
                "HTTP {status} from {}: {text}",
                self.provider_name
            )));
        }

        let parser = OpenAiSseParser::new();
        Ok(unfold_sse_stream(resp, parser))
    }
}

#[derive(Deserialize)]
struct ModelsListResponse {
    data: Option<Vec<ModelEntry>>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

pub async fn discover_openai_models(
    base_url: &str,
    api_key: &str,
    provider_name: &str,
) -> Result<Vec<crate::provider::discovery::DiscoveredModel>, AppError> {
    let base = base_url.trim_end_matches('/').to_string();
    let url = if base.ends_with("/v1") {
        format!("{base}/models")
    } else {
        format!("{base}/v1/models")
    };

    let mut req = HTTP_CLIENT.get(&url);
    if !api_key.trim().is_empty() {
        req = req.header(AUTHORIZATION, format!("Bearer {}", api_key.trim()));
    }

    let resp = req
        .send()
        .await
        .map_err(|e| AppError::Provider(format!("Model discovery failed: {e}")))?;

    if !resp.status().is_success() {
        return Ok(Vec::new());
    }

    let list = resp
        .json::<ModelsListResponse>()
        .await
        .map_err(|e| AppError::Provider(format!("Failed to parse models response: {e}")))?;

    let mut models = Vec::new();
    if let Some(entries) = list.data {
        for entry in entries {
            models.push(crate::provider::discovery::DiscoveredModel {
                name: entry.id.clone(),
                id: entry.id,
                provider: provider_name.to_string(),
                description: format!("{provider_name} model"),
                context_tokens: None,
            });
        }
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_harness_core::model::{
        AssistantContent, ChatMessage, ImageContent, TextContent, ToolCall, ToolFunction, ToolResult, UserContent,
    };

    #[test]
    fn test_convert_openai_user_and_assistant() {
        let user_msg = ChatMessage::User {
            content: vec![
                UserContent::Text(TextContent::new("Hello")),
                UserContent::Image(ImageContent {
                    data: "base64data".to_string(),
                    media_type: Some("image/jpeg".to_string()),
                }),
                UserContent::ToolResult(ToolResult::new("call-1", "bash", "output")),
            ],
        };
        let user_val = convert_chat_message(&user_msg).unwrap();
        assert_eq!(user_val["role"], "tool");
        assert_eq!(user_val["tool_call_id"], "call-1");

        let single_user = ChatMessage::user("plain prompt");
        let single_val = convert_chat_message(&single_user).unwrap();
        assert_eq!(single_val["role"], "user");
        assert_eq!(single_val["content"], "plain prompt");

        let system_msg = ChatMessage::system("system instruction");
        let sys_val = convert_chat_message(&system_msg).unwrap();
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
        let asst_val = convert_chat_message(&asst_msg).unwrap();
        assert_eq!(asst_val["role"], "assistant");
        assert_eq!(asst_val["content"], "I will help");
        assert_eq!(asst_val["tool_calls"][0]["id"], "call-9");
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

        assert_eq!(events.len(), 5);
        assert!(matches!(&events[0], Ok(ModelStreamEvent::Text(t)) if t == "Hello "));
        assert!(matches!(&events[1], Ok(ModelStreamEvent::Text(t)) if t == "world"));
        assert!(
            matches!(&events[2], Ok(ModelStreamEvent::ToolCall(call)) if call.id == "call_1" && call.function.name == "bash")
        );
        assert!(matches!(
            &events[3],
            Ok(ModelStreamEvent::Finish(FinishReason::ToolCalls))
        ));
        assert!(
            matches!(&events[4], Ok(ModelStreamEvent::Usage(u)) if u.total_tokens == 15 && u.cached_input_tokens == Some(2))
        );
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
