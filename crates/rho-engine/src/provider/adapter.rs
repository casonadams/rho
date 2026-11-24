use crate::engine::metrics::StructuralUsage;
use futures::Stream;
use rho_harness_core::error::AppError;
use rho_harness_core::model::{ChatMessage, Reasoning, ToolCall};
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

pub type ModelHandle = Arc<dyn ModelAdapter>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    #[default]
    Stop,
    ToolCalls,
    Length,
    ContentFilter,
    Other,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelCompletionRequest {
    pub preamble: Option<String>,
    pub prompt: String,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ModelToolDefinition>,
    pub max_tokens: Option<u64>,
    pub temperature: Option<f64>,
}

/// Tool-call arguments as streamed by a provider: empty input means no arguments, and text that is not
/// valid JSON is kept as `{"raw": text}` so the tool layer rejects it instead of running on made-up input.
pub fn parse_tool_arguments(raw: &str) -> serde_json::Value {
    if raw.trim().is_empty() {
        return serde_json::json!({});
    }
    serde_json::from_str(raw).unwrap_or_else(|_| serde_json::json!({ "raw": raw }))
}

#[cfg(test)]
impl ModelCompletionRequest {
    /// What the request asked: a one-shot completion's prompt, or the user text a turn ends with
    /// (empty when the turn continues after tool results).
    pub fn asked(&self) -> String {
        if !self.prompt.is_empty() {
            return self.prompt.clone();
        }
        match self.messages.last() {
            Some(ChatMessage::User { content }) => content
                .iter()
                .filter_map(|c| match c {
                    rho_harness_core::model::UserContent::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelCompletionResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: StructuralUsage,
    pub finish_reason: Option<FinishReason>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ModelStreamEvent {
    Text(String),
    Reasoning(String),
    /// A finished reasoning block with the continuity data the provider requires on replay.
    ReasoningBlock(Reasoning),
    ToolCall(ToolCall),
    Usage(StructuralUsage),
    Finish(FinishReason),
}

pub type ModelEventStream = Pin<Box<dyn Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>;

#[async_trait::async_trait]
pub trait ModelAdapter: Send + Sync + std::fmt::Debug {
    fn model_name(&self) -> &str;
    fn provider_name(&self) -> &str;

    /// Streaming providers get a non-streaming completion by draining `stream`.
    async fn complete(&self, request: ModelCompletionRequest) -> Result<ModelCompletionResponse, AppError> {
        let events = futures::StreamExt::collect::<Vec<_>>(self.stream(request).await?).await;
        crate::provider::sse::aggregate_stream_events(events)
    }

    async fn stream(&self, request: ModelCompletionRequest) -> Result<ModelEventStream, AppError>;

    /// This model for short-lived work such as subagents; `None` when it has no cache lifetime to shorten.
    fn for_short_lived_work(&self) -> Option<ModelHandle> {
        None
    }

    /// Request settings besides the model that select a separate prompt cache, such as Claude's thinking.
    fn cache_settings(&self) -> Option<&str> {
        None
    }
}

type MockTurns = Arc<Mutex<Vec<Vec<Result<ModelStreamEvent, AppError>>>>>;

#[derive(Clone, Debug, Default)]
pub struct MockAdapter {
    model: String,
    provider: String,
    turns: MockTurns,
    requests: Arc<Mutex<Vec<ModelCompletionRequest>>>,
}

impl MockAdapter {
    pub fn new(model: impl Into<String>, provider: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            provider: provider.into(),
            turns: Arc::new(Mutex::new(Vec::new())),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn text(response_text: impl Into<String>) -> Self {
        let adapter = Self::new("mock-model", "mock-provider");
        adapter.add_turn(vec![
            Ok(ModelStreamEvent::Text(response_text.into())),
            Ok(ModelStreamEvent::Finish(FinishReason::Stop)),
        ]);
        adapter
    }

    pub fn from_turns(turns: impl IntoIterator<Item = Vec<ModelStreamEvent>>) -> Self {
        let adapter = Self::new("mock-model", "mock-provider");
        for turn in turns {
            let res_turn = turn.into_iter().map(Ok).collect();
            adapter.add_turn(res_turn);
        }
        adapter
    }

    pub fn from_result_turns(turns: impl IntoIterator<Item = Vec<Result<ModelStreamEvent, AppError>>>) -> Self {
        let adapter = Self::new("mock-model", "mock-provider");
        for turn in turns {
            adapter.add_turn(turn);
        }
        adapter
    }

    pub fn add_turn(&self, turn: Vec<Result<ModelStreamEvent, AppError>>) {
        self.turns.lock().unwrap().push(turn);
    }

    pub fn requests(&self) -> Vec<ModelCompletionRequest> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl ModelAdapter for MockAdapter {
    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        &self.provider
    }

    async fn stream(
        &self,
        request: ModelCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>, AppError> {
        self.requests.lock().unwrap().push(request);
        let turn = {
            let mut turns = self.turns.lock().unwrap();
            if turns.is_empty() { Vec::new() } else { turns.remove(0) }
        };
        Ok(Box::pin(futures::stream::iter(turn)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use rho_harness_core::model::ToolFunction;

    #[test]
    fn tool_arguments_distinguish_empty_valid_and_malformed_input() {
        use serde_json::json;
        assert_eq!(parse_tool_arguments(""), json!({}));
        assert_eq!(parse_tool_arguments("  \n"), json!({}));
        assert_eq!(parse_tool_arguments(r#"{"a":1}"#), json!({"a": 1}));
        assert_eq!(
            parse_tool_arguments(r#"{"path": "src/lib"#),
            json!({"raw": r#"{"path": "src/lib"#})
        );
    }

    fn usage() -> StructuralUsage {
        StructuralUsage {
            input_tokens: 10,
            output_tokens: 5,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn complete_drains_the_scripted_turn_into_one_response() {
        let call = ToolCall::new("call-1", ToolFunction::new("test_tool", serde_json::json!({})));
        let adapter = MockAdapter::from_turns([vec![
            ModelStreamEvent::Text("hello".to_string()),
            ModelStreamEvent::ToolCall(call),
            ModelStreamEvent::Usage(usage()),
            ModelStreamEvent::Finish(FinishReason::ToolCalls),
        ]]);
        let request = ModelCompletionRequest {
            prompt: "world".to_string(),
            ..Default::default()
        };

        let response = adapter.complete(request.clone()).await.unwrap();

        assert_eq!(response.content, "hello");
        assert_eq!(response.tool_calls[0].function.name, "test_tool");
        assert_eq!(response.usage.input_tokens, 10);
        assert_eq!(response.finish_reason, Some(FinishReason::ToolCalls));
        assert_eq!(adapter.requests(), [request]);
    }

    #[tokio::test]
    async fn stream_replays_each_scripted_turn_once_in_order() {
        let adapter = MockAdapter::new("test-model", "test-provider");
        adapter.add_turn(vec![
            Ok(ModelStreamEvent::Reasoning("thinking...".to_string())),
            Ok(ModelStreamEvent::Text("streamed output".to_string())),
            Ok(ModelStreamEvent::Usage(usage())),
        ]);
        assert_eq!(adapter.model_name(), "test-model");
        assert_eq!(adapter.provider_name(), "test-provider");

        let events: Vec<_> = adapter
            .stream(ModelCompletionRequest::default())
            .await
            .unwrap()
            .map(Result::unwrap)
            .collect()
            .await;
        let drained = adapter
            .stream(ModelCompletionRequest::default())
            .await
            .unwrap()
            .count()
            .await;

        assert_eq!(
            events,
            [
                ModelStreamEvent::Reasoning("thinking...".to_string()),
                ModelStreamEvent::Text("streamed output".to_string()),
                ModelStreamEvent::Usage(usage()),
            ]
        );
        assert_eq!(drained, 0);
    }
}
