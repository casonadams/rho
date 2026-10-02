use crate::engine::metrics::StructuralUsage;
use futures::Stream;
use rho_harness_core::error::AppError;
use rho_harness_core::model::{ChatMessage, ToolCall};
use std::pin::Pin;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelCompletionRequest {
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
}

#[derive(Clone, Debug, PartialEq)]
pub enum ModelStreamEvent {
    Text(String),
    Reasoning(String),
    ToolCall(ToolCall),
    Usage(StructuralUsage),
}

#[async_trait::async_trait]
pub trait ModelAdapter: Send + Sync {
    fn model_name(&self) -> &str;
    fn provider_name(&self) -> &str;

    async fn complete(&self, request: ModelCompletionRequest) -> Result<ModelCompletionResponse, AppError>;

    async fn stream(
        &self,
        request: ModelCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>, AppError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use rho_harness_core::model::{ToolCall, ToolFunction};

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
