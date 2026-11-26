//! Anthropic SSE stream event decoding into native streaming representations.

mod wire;

#[cfg(test)]
mod tests;

pub use wire::map_finish_reason;
use wire::{ContentBlockStartPayload, ContentDeltaPayload, SseMessage};

use crate::engine::metrics::StructuralUsage;
use crate::engine::metrics::types::nonzero;
use crate::provider::adapter::{FinishReason, ModelStreamEvent};
use crate::provider::sse::{SseEvents, SseLineDecoder, sse_line_feed};
use rho_harness_core::model::{Reasoning, ReasoningContent, ToolCall, ToolFunction};
use std::collections::HashMap;

#[derive(Default)]
pub struct SseParser {
    decoder: SseLineDecoder,
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_input_tokens: u64,
    cache_read_input_tokens: u64,
    reasoning_tokens: u64,
    stop_reason: FinishReason,
    thinking_open: bool,
    thinking_text: String,
    thinking_signature: Option<String>,
    tool_uses: HashMap<usize, ToolUseState>,
    terminated: bool,
}

struct ToolUseState {
    id: String,
    name: String,
    input_json: String,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    sse_line_feed!();

    fn interpret_line(&mut self, data: &str, events: &mut SseEvents) {
        let Ok(msg) = serde_json::from_str::<SseMessage>(data) else {
            return;
        };
        self.interpret_message(msg, events);
    }

    fn handle_content_block_start(
        &mut self,
        index: usize,
        content_block: ContentBlockStartPayload,
        events: &mut SseEvents,
    ) {
        match content_block {
            ContentBlockStartPayload::Thinking { signature } => {
                self.thinking_open = true;
                self.thinking_text.clear();
                self.thinking_signature = signature;
            }
            ContentBlockStartPayload::RedactedThinking { data } => {
                let block = Reasoning {
                    id: None,
                    content: vec![ReasoningContent::Redacted { data }],
                };
                events.push(Ok(ModelStreamEvent::ReasoningBlock(block)));
            }
            ContentBlockStartPayload::ToolUse { id, name } => {
                let name = crate::claude::request::from_claude_tool_name(&name).to_string();
                events.push(Ok(ModelStreamEvent::ToolInputDelta {
                    id: id.clone(),
                    name: name.clone(),
                    json: String::new(),
                }));
                self.tool_uses.insert(
                    index,
                    ToolUseState {
                        id,
                        name,
                        input_json: String::new(),
                    },
                );
            }
            _ => {}
        }
    }

    fn handle_content_block_delta(&mut self, index: usize, delta: ContentDeltaPayload, events: &mut SseEvents) {
        match delta {
            ContentDeltaPayload::TextDelta { text } => {
                events.push(Ok(ModelStreamEvent::Text(text)));
            }
            ContentDeltaPayload::ThinkingDelta { thinking } => {
                self.thinking_text.push_str(&thinking);
                events.push(Ok(ModelStreamEvent::Reasoning(thinking)));
            }
            ContentDeltaPayload::SignatureDelta { signature } => {
                self.thinking_signature
                    .get_or_insert_with(String::new)
                    .push_str(&signature);
            }
            ContentDeltaPayload::InputJsonDelta { partial_json } => {
                if let Some(tool) = self.tool_uses.get_mut(&index) {
                    tool.input_json.push_str(&partial_json);
                    events.push(Ok(ModelStreamEvent::ToolInputDelta {
                        id: tool.id.clone(),
                        name: tool.name.clone(),
                        json: partial_json,
                    }));
                }
            }
            ContentDeltaPayload::Other => {}
        }
    }

    fn handle_message_stop(&mut self, events: &mut SseEvents) {
        let usage = StructuralUsage {
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            total_tokens: self.input_tokens
                + self.cache_read_input_tokens
                + self.cache_creation_input_tokens
                + self.output_tokens,
            cached_input_tokens: nonzero(self.cache_read_input_tokens),
            cache_creation_input_tokens: nonzero(self.cache_creation_input_tokens),
            tool_use_prompt_tokens: None,
            reasoning_tokens: nonzero(self.reasoning_tokens),
        };
        events.push(Ok(ModelStreamEvent::Usage(usage)));
        events.push(Ok(ModelStreamEvent::Finish(self.stop_reason)));
    }

    /// Records the counts a usage block reports; counts it omits keep their earlier values.
    fn apply_usage(&mut self, usage: &wire::MessageUsage) {
        if let Some(input) = usage.input_tokens {
            self.input_tokens = input;
        }
        if let Some(cache_read) = usage.cache_read_input_tokens {
            self.cache_read_input_tokens = cache_read;
        }
        if let Some(cache_write) = usage.cache_creation_input_tokens {
            self.cache_creation_input_tokens = cache_write;
        }
        if let Some(details) = usage.output_tokens_details {
            self.reasoning_tokens = details.thinking_tokens;
        }
    }

    fn handle_message_delta(&mut self, delta: wire::MessageDeltaPayload, usage: Option<wire::MessageUsage>) {
        if let Some(reason) = delta.stop_reason {
            self.stop_reason = map_finish_reason(&reason);
        }
        if let Some(usage) = usage {
            self.output_tokens = usage.output_tokens;
            self.apply_usage(&usage);
        }
    }

    fn interpret_message(&mut self, msg: SseMessage, events: &mut SseEvents) {
        if matches!(msg, SseMessage::MessageStop | SseMessage::Error { .. }) {
            self.terminated = true;
        }
        match msg {
            SseMessage::MessageStart { message } => {
                if let Some(usage) = message.usage {
                    self.apply_usage(&usage);
                }
            }
            SseMessage::ContentBlockStart { index, content_block } => {
                self.handle_content_block_start(index, content_block, events);
            }
            SseMessage::ContentBlockDelta { index, delta } => {
                self.handle_content_block_delta(index, delta, events);
            }
            SseMessage::ContentBlockStop { index } => self.handle_block_stop(index, events),
            SseMessage::MessageDelta { delta, usage } => self.handle_message_delta(delta, usage),
            SseMessage::MessageStop => self.handle_message_stop(events),
            SseMessage::Error { error } => {
                let msg = error.message.unwrap_or_else(|| "Anthropic streaming error".to_string());
                events.push(Err(crate::provider::error::classify_stream_error(
                    error.kind.as_deref(),
                    msg,
                )));
            }
            SseMessage::Ignored => {}
        }
    }

    fn close_thinking_block(&mut self, events: &mut SseEvents) {
        self.thinking_open = false;
        let text = std::mem::take(&mut self.thinking_text);
        let signature = self.thinking_signature.take().filter(|s| !s.is_empty());
        events.push(Ok(ModelStreamEvent::ReasoningBlock(Reasoning::new_with_signature(
            text, signature,
        ))));
    }

    fn close_tool_use_block(&mut self, index: usize, events: &mut SseEvents) {
        let Some(tool) = self.tool_uses.remove(&index) else {
            return;
        };
        let args = crate::provider::adapter::parse_tool_arguments(&tool.input_json);
        let call = ToolCall::new(tool.id, ToolFunction::new(tool.name, args));
        events.push(Ok(ModelStreamEvent::ToolCall(call)));
    }

    fn handle_block_stop(&mut self, index: usize, events: &mut SseEvents) {
        if self.thinking_open {
            self.close_thinking_block(events);
        } else {
            self.close_tool_use_block(index, events);
        }
    }
}

impl crate::provider::sse::SseStreamParser for SseParser {
    fn feed(&mut self, bytes: &[u8]) -> SseEvents {
        SseParser::feed(self, bytes)
    }

    fn finish(&mut self) -> SseEvents {
        if self.terminated {
            return Vec::new();
        }
        vec![Err(rho_harness_core::error::AppError::Network(
            "Claude stream closed before message_stop".to_string(),
        ))]
    }
}
