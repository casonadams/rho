use crate::plugin::capability::{CapabilityError, CapabilityId};
use crate::plugin::contract::{
    FinishReason, MessageContent, MessageRole, ModelMessage, ProviderCapability, ProviderRequest, ProviderStreamEvent,
    ProviderToolDefinition, ScopedCredential,
};
use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Notify;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl ProviderUsage {
    fn add(&mut self, input_tokens: u64, output_tokens: u64) {
        self.input_tokens = self.input_tokens.saturating_add(input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(output_tokens);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuationCheckpoint {
    pub messages: Vec<ModelMessage>,
    pub completed_model_turns: usize,
    pub usage: ProviderUsage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeutralTurnRequest {
    pub model: String,
    pub messages: Vec<ModelMessage>,
    pub credential: Option<ScopedCredential>,
    pub max_output_tokens: Option<u64>,
    pub tools: Vec<ProviderToolDefinition>,
    pub max_turns: usize,
    pub checkpoint: Option<ContinuationCheckpoint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeutralTurnOutput {
    pub text: String,
    pub messages: Vec<ModelMessage>,
    pub usage: ProviderUsage,
    pub finish_reason: FinishReason,
    pub model_turns: usize,
    pub tool_calls: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NeutralTurnTerminal {
    Completed(NeutralTurnOutput),
    Checkpoint(ContinuationCheckpoint),
    Cancelled(ContinuationCheckpoint),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NeutralTurnError {
    #[error("provider operation failed")]
    Provider,
    #[error("provider stream is malformed: {0}")]
    Malformed(&'static str),
    #[error("provider requested unknown tool: {0}")]
    UnknownTool(String),
    #[error("tool operation failed: {0}")]
    Tool(String),
}

#[derive(Clone, Default)]
pub struct CancellationSignal {
    cancelled: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl CancellationSignal {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.notify.notified().await;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeutralToolCall {
    pub call_id: String,
    pub tool_id: CapabilityId,
    pub arguments: Value,
}

#[async_trait]
pub trait NeutralToolExecutor: Send + Sync {
    async fn execute(&self, call: NeutralToolCall) -> Result<NeutralToolResult, NeutralTurnError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeutralToolResult {
    pub content: String,
    pub is_error: bool,
}

pub trait NeutralTurnObserver: Send + Sync {
    fn text_delta(&self, _text: &str) {}
    fn tool_call(&self, _call: &NeutralToolCall) {}
    fn retry(&self, _attempt: u32) {}
}

pub struct NoopTurnObserver;
impl NeutralTurnObserver for NoopTurnObserver {}

pub struct NeutralTurnRuntime<'a> {
    pub provider: &'a dyn ProviderCapability,
    pub tools: &'a dyn NeutralToolExecutor,
    pub observer: &'a dyn NeutralTurnObserver,
    pub cancellation: &'a CancellationSignal,
}

pub async fn run_neutral_turn(
    runtime: NeutralTurnRuntime<'_>,
    request: NeutralTurnRequest,
) -> Result<NeutralTurnTerminal, NeutralTurnError> {
    let NeutralTurnRuntime {
        provider,
        tools,
        observer,
        cancellation,
    } = runtime;
    let mut messages = request.messages;
    let mut usage = ProviderUsage::default();
    let mut completed_model_turns = 0;
    if let Some(checkpoint) = request.checkpoint {
        messages.extend(checkpoint.messages);
        usage = checkpoint.usage;
        completed_model_turns = checkpoint.completed_model_turns;
    }
    let tool_ids = request
        .tools
        .iter()
        .map(|tool| tool.id.clone())
        .collect::<BTreeSet<_>>();
    let mut total_text = String::new();
    let mut tool_calls = 0;

    loop {
        if cancellation.is_cancelled() {
            return Ok(NeutralTurnTerminal::Cancelled(checkpoint(
                messages,
                completed_model_turns,
                usage,
            )));
        }
        if completed_model_turns >= request.max_turns {
            return Ok(NeutralTurnTerminal::Checkpoint(checkpoint(
                messages,
                completed_model_turns,
                usage,
            )));
        }

        let provider_request = ProviderRequest {
            model: request.model.clone(),
            messages: messages.clone(),
            credential: request.credential.clone(),
            max_output_tokens: request.max_output_tokens,
            tools: request.tools.clone(),
        };
        let mut stream = provider.stream(provider_request).await.map_err(map_provider_error)?;
        let mut turn_text = String::new();
        let mut complete_calls = BTreeMap::<String, (CapabilityId, Value)>::new();
        let mut partial_calls = BTreeMap::<String, (CapabilityId, String)>::new();
        let mut finish = None;

        loop {
            let event = tokio::select! {
                _ = cancellation.cancelled() => {
                    return Ok(NeutralTurnTerminal::Cancelled(checkpoint(messages, completed_model_turns, usage)));
                }
                event = stream.next() => event,
            };
            let Some(event) = event else { break };
            let event = event.map_err(map_provider_error)?;
            if finish.is_some() {
                return Err(NeutralTurnError::Malformed("event arrived after terminal finish"));
            }
            match event {
                ProviderStreamEvent::TextDelta { text } => {
                    observer.text_delta(&text);
                    turn_text.push_str(&text);
                }
                ProviderStreamEvent::ToolCallDelta {
                    call_id,
                    tool_id,
                    arguments_delta,
                } => {
                    validate_call_identity(&call_id, &tool_id, &tool_ids)?;
                    if complete_calls.contains_key(&call_id) {
                        return Err(NeutralTurnError::Malformed("tool-call delta followed a completed call"));
                    }
                    let entry = partial_calls
                        .entry(call_id)
                        .or_insert_with(|| (tool_id.clone(), String::new()));
                    if entry.0 != tool_id {
                        return Err(NeutralTurnError::Malformed("tool-call delta correlation changed"));
                    }
                    entry.1.push_str(&arguments_delta);
                }
                ProviderStreamEvent::ToolCall {
                    call_id,
                    tool_id,
                    arguments,
                } => {
                    validate_call_identity(&call_id, &tool_id, &tool_ids)?;
                    if complete_calls.insert(call_id.clone(), (tool_id, arguments)).is_some() {
                        return Err(NeutralTurnError::Malformed("duplicate tool-call identifier"));
                    }
                    partial_calls.remove(&call_id);
                }
                ProviderStreamEvent::Usage {
                    input_tokens,
                    output_tokens,
                } => usage.add(input_tokens, output_tokens),
                ProviderStreamEvent::Finished { reason } => finish = Some(reason),
            }
        }
        completed_model_turns += 1;
        let reason = finish.ok_or(NeutralTurnError::Malformed("stream ended without a finish reason"))?;

        for (call_id, (tool_id, arguments_json)) in partial_calls {
            if complete_calls.contains_key(&call_id) {
                return Err(NeutralTurnError::Malformed("tool call was emitted twice"));
            }
            let arguments = serde_json::from_str(&arguments_json)
                .map_err(|_| NeutralTurnError::Malformed("tool-call arguments are not valid JSON"))?;
            complete_calls.insert(call_id, (tool_id, arguments));
        }

        let mut assistant_content = Vec::new();
        if !turn_text.is_empty() {
            total_text.push_str(&turn_text);
            assistant_content.push(MessageContent::Text { text: turn_text });
        }
        for (call_id, (tool_id, arguments)) in &complete_calls {
            observer.tool_call(&NeutralToolCall {
                call_id: call_id.clone(),
                tool_id: tool_id.clone(),
                arguments: arguments.clone(),
            });
            assistant_content.push(MessageContent::ToolCall {
                call_id: call_id.clone(),
                tool_id: tool_id.clone(),
                arguments: arguments.clone(),
            });
        }
        if !assistant_content.is_empty() {
            messages.push(ModelMessage {
                role: MessageRole::Assistant,
                content: assistant_content,
            });
        }

        if complete_calls.is_empty() {
            if reason == FinishReason::ToolCalls {
                return Err(NeutralTurnError::Malformed("tool-call finish contained no tool calls"));
            }
            return Ok(NeutralTurnTerminal::Completed(NeutralTurnOutput {
                text: total_text,
                messages,
                usage,
                finish_reason: reason,
                model_turns: completed_model_turns,
                tool_calls,
            }));
        }
        if !matches!(reason, FinishReason::ToolCalls | FinishReason::Stop) {
            return Err(NeutralTurnError::Malformed(
                "terminal reason cannot continue with tool calls",
            ));
        }

        for (call_id, (tool_id, arguments)) in complete_calls {
            let result = tools
                .execute(NeutralToolCall {
                    call_id: call_id.clone(),
                    tool_id,
                    arguments,
                })
                .await?;
            tool_calls += 1;
            messages.push(ModelMessage {
                role: MessageRole::Tool,
                content: vec![MessageContent::ToolResult {
                    call_id,
                    content: result.content,
                    is_error: result.is_error,
                }],
            });
        }
    }
}

fn checkpoint(
    messages: Vec<ModelMessage>,
    completed_model_turns: usize,
    usage: ProviderUsage,
) -> ContinuationCheckpoint {
    ContinuationCheckpoint {
        messages,
        completed_model_turns,
        usage,
    }
}

fn validate_call_identity(
    call_id: &str,
    tool_id: &CapabilityId,
    tools: &BTreeSet<CapabilityId>,
) -> Result<(), NeutralTurnError> {
    if call_id.trim().is_empty() {
        return Err(NeutralTurnError::Malformed("tool-call identifier is empty"));
    }
    if !tools.contains(tool_id) {
        return Err(NeutralTurnError::UnknownTool(tool_id.to_string()));
    }
    Ok(())
}

fn map_provider_error(error: CapabilityError) -> NeutralTurnError {
    match error {
        CapabilityError::Cancelled => NeutralTurnError::Provider,
        CapabilityError::InvalidRequest { .. }
        | CapabilityError::PermissionDenied { .. }
        | CapabilityError::Unavailable { .. }
        | CapabilityError::Failed { .. } => NeutralTurnError::Provider,
    }
}

#[cfg(test)]
#[path = "host_loop_tests.rs"]
mod tests;
