use crate::engine::metrics::StructuralUsage;
use futures::Stream;
use rho_harness_core::error::AppError;
use rho_harness_core::model::{ChatMessage, ToolCall};
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
    ToolCall(ToolCall),
    Usage(StructuralUsage),
    Finish(FinishReason),
}

#[async_trait::async_trait]
pub trait ModelAdapter: Send + Sync + std::fmt::Debug {
    fn model_name(&self) -> &str;
    fn provider_name(&self) -> &str;

    async fn complete(&self, request: ModelCompletionRequest) -> Result<ModelCompletionResponse, AppError>;

    async fn stream(
        &self,
        request: ModelCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>, AppError>;
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

    async fn complete(&self, request: ModelCompletionRequest) -> Result<ModelCompletionResponse, AppError> {
        self.requests.lock().unwrap().push(request);
        let turn = {
            let mut turns = self.turns.lock().unwrap();
            if turns.is_empty() { Vec::new() } else { turns.remove(0) }
        };

        let mut content = String::new();
        let mut tool_calls = Vec::new();
        let mut usage = StructuralUsage::default();
        let mut finish_reason = None;

        for event_res in turn {
            let event = event_res?;
            match event {
                ModelStreamEvent::Text(s) => content.push_str(&s),
                ModelStreamEvent::ToolCall(tc) => tool_calls.push(tc),
                ModelStreamEvent::Usage(u) => usage = usage.merge(&u),
                ModelStreamEvent::Finish(f) => finish_reason = Some(f),
                ModelStreamEvent::Reasoning(_) => {}
            }
        }

        Ok(ModelCompletionResponse {
            content,
            tool_calls,
            usage,
            finish_reason,
        })
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
    use rho_harness_core::model::{ToolCall, ToolFunction};

    #[derive(Debug)]
    struct MockAdapter {
        model: String,
        provider: String,
        response_text: String,
    }

    #[async_trait::async_trait]
    impl ModelAdapter for MockAdapter {
        fn model_name(&self) -> &str {
            &self.model
        }

        fn provider_name(&self) -> &str {
            &self.provider
        }

        async fn complete(&self, request: ModelCompletionRequest) -> Result<ModelCompletionResponse, AppError> {
            Ok(ModelCompletionResponse {
                content: format!("{}: {}", self.response_text, request.prompt),
                tool_calls: vec![ToolCall::new(
                    "call-1",
                    ToolFunction::new("test_tool", serde_json::json!({})),
                )],
                usage: StructuralUsage {
                    input_tokens: 10,
                    output_tokens: 5,
                    ..Default::default()
                },
                finish_reason: None,
            })
        }

        async fn stream(
            &self,
            _request: ModelCompletionRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>, AppError> {
            let events = vec![
                Ok(ModelStreamEvent::Reasoning("thinking...".to_string())),
                Ok(ModelStreamEvent::Text(self.response_text.clone())),
                Ok(ModelStreamEvent::Usage(StructuralUsage {
                    input_tokens: 10,
                    output_tokens: 5,
                    ..Default::default()
                })),
            ];
            Ok(Box::pin(futures::stream::iter(events)))
        }
    }

    #[tokio::test]
    async fn test_model_adapter_complete() {
        let adapter = MockAdapter {
            model: "test-model".to_string(),
            provider: "test-provider".to_string(),
            response_text: "hello".to_string(),
        };

        assert_eq!(adapter.model_name(), "test-model");
        assert_eq!(adapter.provider_name(), "test-provider");

        let req = ModelCompletionRequest {
            preamble: None,
            prompt: "world".to_string(),
            messages: vec![ChatMessage::user("world")],
            tools: vec![],
            max_tokens: Some(100),
            temperature: Some(0.7),
        };

        let res = adapter.complete(req).await.unwrap();
        assert_eq!(res.content, "hello: world");
        assert_eq!(res.tool_calls.len(), 1);
        assert_eq!(res.tool_calls[0].function.name, "test_tool");
        assert_eq!(res.usage.input_tokens, 10);
    }

    #[tokio::test]
    async fn test_model_adapter_stream() {
        let adapter = MockAdapter {
            model: "test-model".to_string(),
            provider: "test-provider".to_string(),
            response_text: "streamed output".to_string(),
        };

        let req = ModelCompletionRequest::default();
        let mut stream = adapter.stream(req).await.unwrap();

        let event1 = stream.next().await.unwrap().unwrap();
        assert_eq!(event1, ModelStreamEvent::Reasoning("thinking...".to_string()));

        let event2 = stream.next().await.unwrap().unwrap();
        assert_eq!(event2, ModelStreamEvent::Text("streamed output".to_string()));

        let event3 = stream.next().await.unwrap().unwrap();
        assert!(matches!(event3, ModelStreamEvent::Usage(_)));

        assert!(stream.next().await.is_none());
    }
}
