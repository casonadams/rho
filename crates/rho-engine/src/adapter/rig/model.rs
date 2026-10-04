use futures::StreamExt;
use rig::completion::{CompletionRequest, Usage};
use rig::driver::{DynModel, Exchange, Model, Opened, Opening, Transport};
use rig::error::{EncodeError, ProviderError};
use rig::message::{CallId, ToolName};
use rig::operation::{CallPart, Completion, Finish, TextPart};
use rig::wire::{Decoder, Descriptor, Flow, Mode, Out, Wire, WireEvent};
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::adapter::rig::from_rig_message;
use crate::provider::adapter::{ModelAdapter, ModelCompletionRequest, ModelStreamEvent, ModelToolDefinition};

#[derive(Debug, Clone)]
pub enum AdapterFrame {
    Text(String),
    Reasoning(String),
    ToolCall {
        id: String,
        name: String,
        arguments: String,
        signature: Option<String>,
    },
    Done {
        usage: Usage,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdapterWire {
    pub name: String,
}

#[derive(Default)]
pub struct AdapterDecoder<'id> {
    text: Option<TextPart<'id>>,
    calls: BTreeMap<String, CallPart<'id>>,
}

impl<'id> Decoder<'id, Completion, AdapterFrame> for AdapterDecoder<'id> {
    type Event = AdapterFrame;

    fn classify(&self, frame: AdapterFrame) -> WireEvent<AdapterFrame> {
        WireEvent::Known(frame)
    }

    fn decode(&mut self, frame: AdapterFrame, mut out: Out<'id, Completion>) -> Result<Flow, ProviderError> {
        match frame {
            AdapterFrame::Text(text) => {
                let part = self.text.get_or_insert_with(|| out.text());
                out.push_text(part, &text);
            }
            AdapterFrame::Reasoning(reasoning) => {
                let part = out.reasoning();
                out.push_reasoning(&part, &reasoning);
                out.close_reasoning(part, Default::default());
            }
            AdapterFrame::ToolCall {
                id,
                name,
                arguments,
                signature,
            } => {
                let tool_name = ToolName::new(name).map_err(|e| ProviderError::Provider(e.to_string()))?;
                let part = out.call(CallId::from_wire(id), tool_name)?;
                if signature.is_some() {
                    out.decorate_call(&part, signature, None);
                }
                out.push_arguments(&part, &arguments);
                out.close_call(part)?;
            }
            AdapterFrame::Done { usage } => {
                if let Some(part) = self.text.take() {
                    out.close_text(part);
                }
                for (_, part) in std::mem::take(&mut self.calls) {
                    out.close_call(part)?;
                }
                return Ok(out.end(Finish {
                    usage,
                    ..Finish::default()
                }));
            }
        }
        Ok(Flow::More)
    }
}

impl Wire for AdapterWire {
    type Op = Completion;
    type Payload = CompletionRequest;
    type Frame = AdapterFrame;
    type Decoder<'id> = AdapterDecoder<'id>;

    fn describe(&self) -> Descriptor<'_> {
        Descriptor::new(&self.name)
    }

    fn encode(&self, request: CompletionRequest, _mode: Mode) -> Result<CompletionRequest, EncodeError> {
        Ok(request)
    }

    fn decoder<'id>(&self) -> AdapterDecoder<'id> {
        AdapterDecoder::default()
    }
}

#[derive(Clone)]
pub struct AdapterTransport {
    adapter: Arc<dyn ModelAdapter>,
}

impl Transport<AdapterWire> for AdapterTransport {
    fn send(&self, payload: CompletionRequest, _exchange: Exchange) -> Opening<AdapterFrame> {
        let adapter = self.adapter.clone();
        Opening::new(async move {
            let rho_req = to_rho_request(&payload);
            let stream = adapter
                .stream(rho_req)
                .await
                .map_err(|e| ProviderError::Provider(e.to_string()))?;

            let mapped = futures::stream::unfold(stream, |mut s| async move {
                let item = s.next().await?;
                let frame = match item {
                    Ok(ModelStreamEvent::Text(t)) => Ok(AdapterFrame::Text(t)),
                    Ok(ModelStreamEvent::Reasoning(r)) => Ok(AdapterFrame::Reasoning(r)),
                    Ok(ModelStreamEvent::ToolCall(call)) => Ok(AdapterFrame::ToolCall {
                        id: call.id,
                        name: call.function.name,
                        arguments: call.function.arguments.to_string(),
                        signature: call.signature,
                    }),
                    Ok(ModelStreamEvent::Usage(u)) => Ok(AdapterFrame::Done {
                        usage: make_rig_usage(&u),
                    }),
                    Err(e) => Err(ProviderError::Provider(e.to_string())),
                };
                Some((frame, s))
            });

            Ok(Opened::new(mapped))
        })
    }
}

fn to_rho_request(request: &CompletionRequest) -> ModelCompletionRequest {
    let messages = request.chat_history.iter().map(from_rig_message).collect();
    let prompt = request
        .chat_history
        .last()
        .map(|m| match m {
            rig::message::Message::User { content } => content
                .iter()
                .filter_map(|c| match c {
                    rig::message::UserContent::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        })
        .unwrap_or_default();
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
        prompt,
        messages,
        tools,
        max_tokens: request.max_tokens,
        temperature: request.temperature,
    }
}

fn make_rig_usage(u: &crate::engine::metrics::StructuralUsage) -> Usage {
    Usage {
        input_tokens: Some(u.input_tokens),
        output_tokens: Some(u.output_tokens),
        total_tokens: Some(u.total_tokens),
        cached_input_tokens: u.cached_input_tokens,
        cache_creation_input_tokens: u.cache_creation_input_tokens,
        reasoning_tokens: u.reasoning_tokens,
        ..Default::default()
    }
}

pub fn into_dyn_model<M: ModelAdapter + 'static>(adapter: M) -> DynModel<Completion> {
    let name = adapter.provider_name().to_string();
    let wire = AdapterWire { name };
    let transport = AdapterTransport {
        adapter: Arc::new(adapter),
    };
    Model::new(wire, transport).erase()
}

pub fn into_dyn_model_arc(adapter: Arc<dyn ModelAdapter>) -> DynModel<Completion> {
    let name = adapter.provider_name().to_string();
    let wire = AdapterWire { name };
    let transport = AdapterTransport { adapter };
    Model::new(wire, transport).erase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::metrics::StructuralUsage;
    use crate::provider::adapter::ModelCompletionResponse;
    use futures::Stream;
    use rho_harness_core::error::AppError;
    use rho_harness_core::model::{ToolCall, ToolFunction};
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
    async fn test_rig_model_adapter_dyn_model() {
        let dyn_model = into_dyn_model(TestAdapter);
        let req = CompletionRequest::new(rig::message::Message::user("hello"));
        let resp = dyn_model.call(req).await.unwrap();
        assert_eq!(resp.choice.len(), 1);
        assert_eq!(resp.usage.input_tokens, Some(50));
        assert_eq!(resp.usage.output_tokens, Some(25));
    }
}
