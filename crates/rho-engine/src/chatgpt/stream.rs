//! OpenAI Responses API SSE streaming parser for ChatGPT (Codex).
//!
//! Handles `response.output_text.delta`, `response.reasoning_summary_part.added`,
//! `response.reasoning_summary_text.delta`, function calls, and `response.completed`.
//! Crucially, this parser injects `\n\n` paragraph boundaries when new reasoning summary
//! parts are added (`summary_index > 0`), ensuring discrete thinking steps do not collide.

#[cfg(test)]
mod tests;

use crate::provider::sse::{SseLineDecoder, SseStreamParser};
use rig::completion::{CompletionError, FinishReason, Usage};
use rig::streaming::{MintKind, RawStreamingChoice, RawStreamingToolCall, StreamFinal, StreamPartId};
use serde::Deserialize;
use std::collections::HashMap;

const REASONING_ID: StreamPartId = StreamPartId::minted(MintKind::Reasoning, 0);

pub type SseEvents = Vec<Result<RawStreamingChoice<StreamFinal>, CompletionError>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SummaryPartCoord {
    output_index: Option<u64>,
    summary_index: u64,
}

#[derive(Default)]
pub struct SseParser {
    decoder: SseLineDecoder,
    has_reasoning_content: bool,
    reasoning_trailing_newline: bool,
    current_output_index: Option<u64>,
    active_summary_part: Option<SummaryPartCoord>,
    pending_tool_calls: HashMap<String, PendingToolCall>,
}

struct PendingToolCall {
    id: String,
    call_id: String,
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ResponsesWireEvent {
    #[serde(rename = "response.output_text.delta")]
    OutputTextDelta { delta: String },
    #[serde(rename = "response.reasoning_summary_part.added")]
    ReasoningSummaryPartAdded {
        #[serde(default)]
        summary_index: u64,
        #[serde(default)]
        output_index: Option<u64>,
    },
    #[serde(rename = "response.reasoning_summary_text.delta")]
    ReasoningSummaryTextDelta {
        delta: String,
        #[serde(default)]
        summary_index: u64,
        #[serde(default)]
        output_index: Option<u64>,
    },
    #[serde(rename = "response.reasoning_summary.delta")]
    ReasoningSummaryDelta {
        delta: String,
        #[serde(default)]
        summary_index: u64,
        #[serde(default)]
        output_index: Option<u64>,
    },
    #[serde(rename = "response.reasoning_summary_part.done")]
    ReasoningSummaryPartDone,
    #[serde(rename = "response.reasoning_summary_text.done")]
    ReasoningSummaryTextDone,
    #[serde(rename = "response.reasoning_summary.done")]
    ReasoningSummaryDone,
    #[serde(rename = "response.reasoning_text.delta")]
    ReasoningTextDelta { delta: String },
    #[serde(rename = "response.output_item.added")]
    OutputItemAdded {
        item: OutputItemPayload,
        #[serde(default)]
        output_index: u64,
    },
    #[serde(rename = "response.function_call_arguments.delta")]
    FunctionCallArgsDelta {
        delta: String,
        #[serde(default)]
        item_id: Option<String>,
        #[serde(default)]
        output_index: Option<u64>,
    },
    #[serde(rename = "response.output_item.done")]
    OutputItemDone {
        item: OutputItemPayload,
        #[serde(default)]
        output_index: u64,
    },
    #[serde(rename = "response.completed")]
    Completed { response: CompletedResponsePayload },
    #[serde(rename = "response.failed")]
    Failed { response: FailedResponsePayload },
    #[serde(rename = "error")]
    Error { error: ErrorDetailsPayload },
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
struct FunctionCallPayload {
    #[serde(default)]
    id: String,
    #[serde(default)]
    call_id: String,
    name: String,
    #[serde(default)]
    arguments: String,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum OutputItemPayload {
    #[serde(rename = "function_call")]
    FunctionCall(FunctionCallPayload),
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct CompletedResponsePayload {
    #[serde(default)]
    usage: Option<ResponsesUsagePayload>,
}

#[derive(Deserialize)]
struct FailedResponsePayload {
    #[serde(default)]
    error: Option<ErrorDetailsPayload>,
}

#[derive(Deserialize)]
struct ErrorDetailsPayload {
    #[serde(default)]
    message: String,
}

#[derive(Deserialize, Default)]
struct ResponsesUsagePayload {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    total_tokens: Option<u64>,
    #[serde(default)]
    input_tokens_details: Option<InputTokensDetailsPayload>,
    #[serde(default)]
    output_tokens_details: Option<OutputTokensDetailsPayload>,
}

#[derive(Deserialize, Default)]
struct InputTokensDetailsPayload {
    #[serde(default)]
    cached_tokens: Option<u64>,
}

#[derive(Deserialize, Default)]
struct OutputTokensDetailsPayload {
    #[serde(default)]
    reasoning_tokens: Option<u64>,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, bytes: &[u8]) -> SseEvents {
        SseStreamParser::feed(self, bytes)
    }

    fn start_reasoning_part(&mut self, coord: SummaryPartCoord, events: &mut SseEvents) {
        if self.active_summary_part != Some(coord) {
            if self.has_reasoning_content && !self.reasoning_trailing_newline {
                events.push(Ok(RawStreamingChoice::ReasoningDelta {
                    id: REASONING_ID,
                    provider_id: None,
                    reasoning: "\n\n".to_string(),
                }));
                self.reasoning_trailing_newline = true;
            }
            self.active_summary_part = Some(coord);
        }
    }

    fn handle_reasoning_part_added(&mut self, output_index: Option<u64>, summary_index: u64, events: &mut SseEvents) {
        let coord = SummaryPartCoord {
            output_index: output_index.or(self.current_output_index),
            summary_index,
        };
        self.start_reasoning_part(coord, events);
    }

    fn handle_reasoning_delta(
        &mut self,
        delta: String,
        output_index: Option<u64>,
        summary_index: Option<u64>,
        events: &mut SseEvents,
    ) {
        if let Some(s_idx) = summary_index {
            self.handle_reasoning_part_added(output_index, s_idx, events);
        }
        if delta.is_empty() {
            return;
        }
        self.has_reasoning_content = true;
        self.reasoning_trailing_newline = delta.ends_with('\n');
        events.push(Ok(RawStreamingChoice::ReasoningDelta {
            id: REASONING_ID,
            provider_id: None,
            reasoning: delta,
        }));
    }

    fn handle_function_call_added(&mut self, call: FunctionCallPayload, index: u64) {
        let key = if !call.id.is_empty() {
            call.id.clone()
        } else {
            format!("output-{index}")
        };
        self.pending_tool_calls.insert(
            key,
            PendingToolCall {
                id: call.id,
                call_id: call.call_id,
                name: call.name,
                arguments: call.arguments,
            },
        );
    }

    fn handle_function_call_args_delta(&mut self, delta: String, item_id: Option<String>, output_index: Option<u64>) {
        let key = item_id.or_else(|| output_index.map(|i| format!("output-{i}")));
        if let Some(key) = key
            && let Some(tool) = self.pending_tool_calls.get_mut(&key)
        {
            tool.arguments.push_str(&delta);
        }
    }

    fn handle_function_call_done(&mut self, call: FunctionCallPayload, index: u64, events: &mut SseEvents) {
        let key = if !call.id.is_empty() {
            call.id.clone()
        } else {
            format!("output-{index}")
        };
        let (name, final_args, call_id, wire_id) = if let Some(pending) = self.pending_tool_calls.remove(&key) {
            let args_str = if !call.arguments.is_empty() {
                call.arguments
            } else {
                pending.arguments
            };
            let cid = if !call.call_id.is_empty() {
                call.call_id
            } else {
                pending.call_id
            };
            let wid = if !call.id.is_empty() { call.id } else { pending.id };
            (pending.name, args_str, cid, wid)
        } else {
            (call.name, call.arguments, call.call_id, call.id)
        };

        let parsed_args = if final_args.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(&final_args).unwrap_or_else(|_| serde_json::json!({}))
        };

        let effective_id = if !call_id.is_empty() {
            call_id.clone()
        } else if !wire_id.is_empty() {
            wire_id
        } else {
            format!("call-{index}")
        };

        let mut tc = RawStreamingToolCall::new(StreamPartId::wire(effective_id), name, parsed_args);
        if !call_id.is_empty() {
            tc.call_id = Some(call_id);
        }
        events.push(Ok(RawStreamingChoice::ToolCall(tc)));
    }

    fn handle_completed(&mut self, usage_payload: Option<ResponsesUsagePayload>, events: &mut SseEvents) {
        let usage = usage_payload
            .map(|u| Usage {
                input_tokens: u.input_tokens.unwrap_or(0),
                output_tokens: u.output_tokens.unwrap_or(0),
                total_tokens: u.total_tokens.unwrap_or(0),
                cached_input_tokens: u.input_tokens_details.and_then(|d| d.cached_tokens).unwrap_or(0),
                reasoning_tokens: u.output_tokens_details.and_then(|d| d.reasoning_tokens).unwrap_or(0),
                ..Usage::new()
            })
            .unwrap_or_default();

        let final_response = StreamFinal::new("chatgpt", usage).with_finish_reason(FinishReason::Stop);
        events.push(Ok(RawStreamingChoice::FinalResponse(final_response)));
    }

    fn interpret_line(&mut self, data: &str, events: &mut SseEvents) {
        let Ok(event) = serde_json::from_str::<ResponsesWireEvent>(data) else {
            return;
        };
        match event {
            ResponsesWireEvent::OutputTextDelta { delta } => {
                events.push(Ok(RawStreamingChoice::Message(delta)));
            }
            ResponsesWireEvent::ReasoningSummaryPartAdded {
                summary_index,
                output_index,
            } => {
                self.handle_reasoning_part_added(output_index, summary_index, events);
            }
            ResponsesWireEvent::ReasoningSummaryTextDelta {
                delta,
                summary_index,
                output_index,
            }
            | ResponsesWireEvent::ReasoningSummaryDelta {
                delta,
                summary_index,
                output_index,
            } => {
                self.handle_reasoning_delta(delta, output_index, Some(summary_index), events);
            }
            ResponsesWireEvent::ReasoningSummaryPartDone
            | ResponsesWireEvent::ReasoningSummaryTextDone
            | ResponsesWireEvent::ReasoningSummaryDone => {
                self.active_summary_part = None;
            }
            ResponsesWireEvent::ReasoningTextDelta { delta } => {
                self.handle_reasoning_delta(delta, None, None, events);
            }
            ResponsesWireEvent::OutputItemAdded { item, output_index } => {
                self.current_output_index = Some(output_index);
                self.active_summary_part = None;
                if let OutputItemPayload::FunctionCall(call) = item {
                    self.handle_function_call_added(call, output_index);
                }
            }
            ResponsesWireEvent::FunctionCallArgsDelta {
                delta,
                item_id,
                output_index,
            } => {
                self.handle_function_call_args_delta(delta, item_id, output_index);
            }
            ResponsesWireEvent::OutputItemDone { item, output_index } => {
                if let OutputItemPayload::FunctionCall(call) = item {
                    self.handle_function_call_done(call, output_index, events);
                } else {
                    self.active_summary_part = None;
                }
            }
            ResponsesWireEvent::Completed { response } => {
                self.handle_completed(response.usage, events);
            }
            ResponsesWireEvent::Failed { response } => {
                let msg = response
                    .error
                    .map(|e| e.message)
                    .unwrap_or_else(|| "ChatGPT generation failed".to_string());
                events.push(Err(CompletionError::ProviderError(msg)));
            }
            ResponsesWireEvent::Error { error } => {
                events.push(Err(CompletionError::ProviderError(error.message)));
            }
            ResponsesWireEvent::Unknown => {}
        }
    }
}

impl SseStreamParser for SseParser {
    fn feed(&mut self, bytes: &[u8]) -> SseEvents {
        let mut events = Vec::new();
        for line in self.decoder.decode_lines(bytes) {
            self.interpret_line(&line, &mut events);
        }
        events
    }
}
