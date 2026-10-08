use futures::StreamExt;
use rig::completion::{CompletionRequest, Usage};
use rig::driver::{DynModel, Exchange, Model, Opened, Opening, Transport};
use rig::error::{EncodeError, ProviderError};
use rig::message::{CallId, ToolName};
use rig::operation::{Block, Completion, Finish};
use rig::wire::{Decoder, Descriptor, Flow, Mode, Out, Wire, WireEvent};
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
pub struct AdapterReassembler;

impl rig::wire::document::Serves<Completion> for AdapterReassembler {}

impl rig::wire::document::Reassemble<AdapterFrame> for AdapterReassembler {
    fn absorb(&mut self, _frame: &AdapterFrame) {}

    fn finish(self) -> serde_json::Value {
        serde_json::Value::Null
    }
}

#[derive(Default)]
pub struct AdapterDecoder<'id> {
    next_index: usize,
    text_index: Option<usize>,
    _phantom: std::marker::PhantomData<&'id ()>,
}

impl<'id> Decoder<'id, Completion, AdapterFrame> for AdapterDecoder<'id> {
    type Event = AdapterFrame;

    fn classify(&self, frame: AdapterFrame) -> WireEvent<AdapterFrame> {
        WireEvent::Known(frame)
    }

    fn decode(&mut self, frame: AdapterFrame, mut out: Out<'id, Completion>) -> Result<Flow, ProviderError> {
        match frame {
            AdapterFrame::Text(text) => {
                let idx = match self.text_index {
                    Some(i) => i,
                    None => {
                        let i = self.next_index;
                        self.next_index += 1;
                        out.open(i, Block::Text, serde_json::Value::Null)?;
                        self.text_index = Some(i);
                        i
                    }
                };
                out.push(idx, &text)?;
            }
            AdapterFrame::Reasoning(reasoning) => {
                let idx = self.next_index;
                self.next_index += 1;
                out.whole(
                    idx,
                    Block::Reasoning { redacted: false },
                    serde_json::Value::Null,
                    &reasoning,
                )?;
            }
            AdapterFrame::ToolCall {
                id,
                name,
                arguments,
                signature,
            } => {
                let tool_name = ToolName::new(name).map_err(|e| ProviderError::Provider(e.to_string()))?;
                let idx = self.next_index;
                self.next_index += 1;
                let native = signature
                    .map(|s| serde_json::json!({ "signature": s }))
                    .unwrap_or(serde_json::Value::Null);
                out.open(
                    idx,
                    Block::Call {
                        id: CallId::from_wire(id),
                        name: tool_name,
                    },
                    native,
                )?;
                out.push(idx, &arguments)?;
                out.finish(idx)?;
            }
            AdapterFrame::Done { usage } => {
                out.finish_open()?;
                return Ok(out.end(Finish {
                    usage,
                    reason: Some(rig::completion::FinishReason::Stop),
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
    type Reassembler = AdapterReassembler;

    fn describe(&self) -> Descriptor<'_> {
        Descriptor::new(&self.name).replay(self)
    }

    fn encode(&self, request: CompletionRequest, _mode: Mode) -> Result<CompletionRequest, EncodeError> {
        Ok(request)
    }

    fn decoder<'id>(&self) -> AdapterDecoder<'id> {
        AdapterDecoder::default()
    }
}

impl rig::completion::ReplayTarget for AdapterWire {
    fn api(&self) -> rig::message::Api {
        rig::message::Api::from_static("rho.adapter")
    }

    fn provider(&self) -> &str {
        &self.name
    }

    fn model(&self) -> &str {
        ""
    }

    fn accepts(&self, _model: &str) -> rig::completion::Accepts {
        rig::completion::Accepts::ALL
    }

    fn map_options(
        &self,
        _request: &CompletionRequest,
        fields: rig::completion::options::OptionFields<'_>,
    ) -> rig::completion::options::OptionMap {
        use rig::completion::options::{Mapping, OptionFields, OptionMap};
        let OptionFields {
            reasoning,
            cache,
            service_tier,
            verbosity,
            parallel_tool_calls,
            top_p,
            seed,
            stop,
        } = fields;
        let mapped = |set: bool| match set {
            true => Mapping::unsupported("adapter does not map options"),
            false => Mapping::Nothing,
        };
        OptionMap {
            reasoning: mapped(reasoning.is_some()),
            cache: mapped(cache.is_some()),
            service_tier: mapped(service_tier.is_some()),
            verbosity: mapped(verbosity.is_some()),
            parallel_tool_calls: mapped(parallel_tool_calls.is_some()),
            top_p: mapped(top_p.is_some()),
            seed: mapped(seed.is_some()),
            stop: mapped(!stop.is_empty()),
        }
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
            name: t.name.to_string(),
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
    let mut usage = Usage::new();
    usage.input_tokens = Some(u.input_tokens);
    usage.output_tokens = Some(u.output_tokens);
    usage.total_tokens = Some(u.total_tokens);
    usage.cached_input_tokens = u.cached_input_tokens;
    usage.cache_creation_input_tokens = u.cache_creation_input_tokens;
    usage.reasoning_tokens = u.reasoning_tokens;
    usage
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

    struct StreamTestAdapter;

    #[async_trait::async_trait]
    impl ModelAdapter for StreamTestAdapter {
        fn model_name(&self) -> &str {
            "stream-test-model"
        }

        fn provider_name(&self) -> &str {
            "stream-test-provider"
        }

        async fn complete(&self, _request: ModelCompletionRequest) -> Result<ModelCompletionResponse, AppError> {
            unimplemented!()
        }

        async fn stream(
            &self,
            _request: ModelCompletionRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>, AppError> {
            let mut tc = ToolCall::new("tc-1", ToolFunction::new("bash", serde_json::json!({"cmd": "ls"})));
            tc.signature = Some("sig-123".to_string());
            let events = vec![
                Ok(ModelStreamEvent::Text("streamed text".to_string())),
                Ok(ModelStreamEvent::Reasoning("thinking".to_string())),
                Ok(ModelStreamEvent::ToolCall(tc)),
                Ok(ModelStreamEvent::ToolCall(ToolCall::new(
                    "tc-2",
                    ToolFunction::new("read", serde_json::json!({"path": "foo"})),
                ))),
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

    #[tokio::test]
    async fn test_rig_model_adapter_dyn_model_stream() {
        use futures::StreamExt;
        let dyn_model = into_dyn_model(StreamTestAdapter);
        let req = CompletionRequest::new(rig::message::Message::user("hello"));
        let mut stream = dyn_model.stream(req).unwrap();
        let mut count = 0;
        while let Some(item) = stream.next().await {
            assert!(item.is_ok());
            count += 1;
        }
        assert!(count > 0);
    }
}
