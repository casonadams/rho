use futures::StreamExt;
use rig::completion::{CompletionError, CompletionModel, CompletionRequest, CompletionResponse, Usage};
use rig::message::{AssistantContent, ToolCall as RigToolCall, ToolCallId, ToolFunction as RigToolFunction};
use rig::streaming::{
    MintKind, RawStreamingChoice, RawStreamingToolCall, StreamFinal, StreamPartId, StreamingCompletionResponse,
};
use std::sync::Arc;

use crate::adapter::rig::from_rig_message;
use crate::provider::adapter::{ModelAdapter, ModelCompletionRequest, ModelStreamEvent, ModelToolDefinition};

#[derive(Clone)]
pub struct RigModelAdapter<M: ModelAdapter> {
    adapter: Arc<M>,
}

impl<M: ModelAdapter> RigModelAdapter<M> {
    pub fn new(adapter: M) -> Self {
        Self {
            adapter: Arc::new(adapter),
        }
    }

    pub fn from_arc(adapter: Arc<M>) -> Self {
        Self { adapter }
    }

    pub fn inner(&self) -> &M {
        &self.adapter
    }
}

fn to_rho_request(request: &CompletionRequest) -> ModelCompletionRequest {
    let messages = request.chat_history.iter().map(from_rig_message).collect();
    let tools = request
        .tools
        .iter()
        .map(|t| ModelToolDefinition {
            name: t.name.clone(),
            description: t.description.clone(),
            parameters: t.parameters.clone(),
        })
        .collect();
    ModelCompletionRequest {
        prompt: request.preamble.as_deref().unwrap_or("").to_string(),
        messages,
        tools,
        max_tokens: request.max_tokens,
        temperature: request.temperature,
    }
}

fn make_rig_usage(u: &crate::engine::metrics::StructuralUsage) -> Usage {
    Usage {
        input_tokens: u.input_tokens,
        output_tokens: u.output_tokens,
        total_tokens: u.total_tokens,
        ..Default::default()
    }
}

impl<M: ModelAdapter + 'static> CompletionModel for RigModelAdapter<M> {
    async fn completion(&self, request: CompletionRequest) -> Result<CompletionResponse, CompletionError> {
        let provider_name = self.adapter.provider_name().to_string();
        let rho_req = to_rho_request(&request);
        let resp = self
            .adapter
            .complete(rho_req)
            .await
            .map_err(|e| CompletionError::ProviderError(e.to_string()))?;

        let mut choice = Vec::new();
        if !resp.content.is_empty() {
            choice.push(AssistantContent::text(resp.content));
        }
        for call in resp.tool_calls {
            choice.push(AssistantContent::ToolCall(RigToolCall::new(
                ToolCallId::new_or_mint(&call.id),
                RigToolFunction::new(call.function.name, call.function.arguments),
            )));
        }

        let usage = make_rig_usage(&resp.usage);
        Ok(CompletionResponse::new(
            choice,
            usage,
            Box::leak(provider_name.into_boxed_str()),
        ))
    }

    async fn stream(&self, request: CompletionRequest) -> Result<StreamingCompletionResponse, CompletionError> {
        let provider_name = self.adapter.provider_name().to_string();
        let static_provider: &'static str = Box::leak(provider_name.into_boxed_str());
        let rho_req = to_rho_request(&request);
        let stream = self
            .adapter
            .stream(rho_req)
            .await
            .map_err(|e| CompletionError::ProviderError(e.to_string()))?;

        let mapped = stream.map(move |event_res| match event_res {
            Ok(ModelStreamEvent::Text(t)) => Ok(RawStreamingChoice::Message(t)),
            Ok(ModelStreamEvent::Reasoning(r)) => Ok(RawStreamingChoice::ReasoningDelta {
                id: StreamPartId::minted(MintKind::Reasoning, 0),
                provider_id: None,
                reasoning: r,
            }),
            Ok(ModelStreamEvent::ToolCall(call)) => Ok(RawStreamingChoice::ToolCall(RawStreamingToolCall::new(
                StreamPartId::wire(call.id),
                call.function.name,
                call.function.arguments,
            ))),
            Ok(ModelStreamEvent::Usage(u)) => {
                let usage = make_rig_usage(&u);
                Ok(RawStreamingChoice::FinalResponse(StreamFinal::new(
                    static_provider,
                    usage,
                )))
            }
            Err(e) => Err(CompletionError::ProviderError(e.to_string())),
        });

        Ok(StreamingCompletionResponse::stream(static_provider, Box::pin(mapped)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::metrics::StructuralUsage;
    use crate::provider::adapter::ModelCompletionResponse;
    use futures::Stream;
    use rho_harness_core::error::AppError;
    use rho_harness_core::model::{ChatMessage, ToolCall, ToolFunction};
    use rig::streaming::StreamedAssistantContent;
    use std::pin::Pin;

    struct TestAdapter;

    #[async_trait::async_trait]
    impl ModelAdapter for TestAdapter {
        fn model_name(&self) -> &str {
            "test-model"
        }

        fn provider_name(&self) -> &str {
            "test-provider"
        }

        async fn complete(&self, request: ModelCompletionRequest) -> Result<ModelCompletionResponse, AppError> {
            Ok(ModelCompletionResponse {
                content: format!("response to: {}", request.prompt),
                tool_calls: vec![ToolCall::new(
                    "tc-1",
                    ToolFunction::new("bash", serde_json::json!({"cmd": "ls"})),
                )],
                usage: StructuralUsage {
                    input_tokens: 50,
                    output_tokens: 25,
                    ..Default::default()
                },
            })
        }

        async fn stream(
            &self,
            _request: ModelCompletionRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>, AppError> {
            let events = vec![
                Ok(ModelStreamEvent::Text("streamed text".to_string())),
                Ok(ModelStreamEvent::Usage(StructuralUsage {
                    input_tokens: 50,
                    output_tokens: 25,
                    ..Default::default()
                })),
            ];
            Ok(Box::pin(futures::stream::iter(events)))
        }
    }

    #[tokio::test]
    async fn test_rig_model_adapter_completion() {
        let adapter = RigModelAdapter::new(TestAdapter);
        let req = CompletionRequest {
            preamble: Some("hello".to_string()),
            chat_history: vec![crate::adapter::rig::to_rig_message(&ChatMessage::user("hello"))],
            documents: vec![],
            tools: vec![],
            temperature: None,
            max_tokens: None,
            additional_params: None,
            model: None,
            output_schema: None,
            record_telemetry_content: false,
            tool_choice: None,
        };

        let resp = adapter.completion(req).await.unwrap();
        assert_eq!(resp.choice.len(), 2);
        assert!(matches!(&resp.choice[0], AssistantContent::Text(t) if t.text == "response to: hello"));
        assert!(matches!(&resp.choice[1], AssistantContent::ToolCall(_)));
        assert_eq!(resp.usage.input_tokens, 50);
        assert_eq!(resp.usage.output_tokens, 25);
    }

    #[tokio::test]
    async fn test_rig_model_adapter_stream() {
        let adapter = RigModelAdapter::new(TestAdapter);
        let req = CompletionRequest {
            preamble: Some("stream me".to_string()),
            chat_history: vec![],
            documents: vec![],
            tools: vec![],
            temperature: None,
            max_tokens: None,
            additional_params: None,
            model: None,
            output_schema: None,
            record_telemetry_content: false,
            tool_choice: None,
        };

        let mut stream_resp = adapter.stream(req).await.unwrap();
        let first = stream_resp.next().await.unwrap().unwrap();
        assert!(matches!(first, StreamedAssistantContent::Text(t) if t.text == "streamed text"));
        let second = stream_resp.next().await.unwrap().unwrap();
        assert!(matches!(second, StreamedAssistantContent::Final(_)));
    }
}
