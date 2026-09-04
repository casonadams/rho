//! Antigravity wire format, stream side: SSE chunk decoding into rig's
//! canonical streaming events.

pub mod wire;

#[cfg(test)]
mod tests;

pub use wire::map_finish_reason;
use wire::{StreamChunk, StreamPart, usage_from_metadata};

use rig::completion::CompletionError;
use rig::streaming::{MintKind, RawStreamingChoice, RawStreamingToolCall, StreamFinal, StreamPartId};
use serde_json::Value;

use super::request::sanitize_tool_call_id;

/// Incremental SSE parser for Antigravity `streamGenerateContent?alt=sse`.
///
/// Feed transport bytes; collect canonical rig events. Reasoning uses the
/// constant minted identity gemini thought parts share (no wire id).
pub struct SseParser {
    buffer: String,
    reasoning_open: bool,
    reasoning_text: String,
    reasoning_signature: Option<String>,
    next_minted_tool: u64,
}

pub type SseEvents = Vec<Result<RawStreamingChoice<StreamFinal>, CompletionError>>;

impl Default for SseParser {
    fn default() -> Self {
        Self::new()
    }
}

const REASONING_ID: StreamPartId = StreamPartId::minted(MintKind::Reasoning, 0);

impl SseParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            reasoning_open: false,
            reasoning_text: String::new(),
            reasoning_signature: None,
            next_minted_tool: 0,
        }
    }

    /// Consume one transport chunk into stream events.
    pub fn feed(&mut self, bytes: &[u8]) -> SseEvents {
        self.buffer.push_str(&String::from_utf8_lossy(bytes));
        let mut events: SseEvents = Vec::new();
        while let Some(newline) = self.buffer.find('\n') {
            let line = self.buffer[..newline].to_string();
            self.buffer.drain(..=newline);
            self.interpret_line(&line, &mut events);
        }
        events
    }

    fn interpret_line(&mut self, line: &str, events: &mut SseEvents) {
        let Some(json_line) = line.strip_prefix("data:") else {
            return;
        };
        let json_line = json_line.trim();
        if json_line.is_empty() || json_line == "[DONE]" {
            return;
        }
        let Ok(chunk) = serde_json::from_str::<StreamChunk>(json_line) else {
            return;
        };
        if let Some(error) = chunk.error {
            let message = match error.message {
                Some(Value::String(text)) => text,
                Some(other) => other.to_string(),
                None => "unknown provider error".to_string(),
            };
            events.push(Err(CompletionError::ProviderError(message)));
            return;
        }
        let body = chunk.response.unwrap_or(chunk.direct);
        for candidate in &body.candidates {
            for part in candidate.content.as_ref().map(|c| c.parts.clone()).unwrap_or_default() {
                self.interpret_part(part, events);
            }
            if let Some(reason) = &candidate.finish_reason {
                let usage = body
                    .usage_metadata
                    .as_ref()
                    .map(usage_from_metadata)
                    .unwrap_or_default();
                self.close_reasoning(events);
                let mut final_response =
                    StreamFinal::new("antigravity", usage).with_finish_reason(map_finish_reason(reason));
                final_response.raw = serde_json::to_value(json_line).unwrap_or(Value::Null);
                events.push(Ok(RawStreamingChoice::FinalResponse(final_response)));
            }
        }
    }

    fn interpret_part(&mut self, part: StreamPart, events: &mut SseEvents) {
        let StreamPart {
            text,
            thought,
            thought_signature,
            function_call,
        } = part;
        if let Some(call) = function_call {
            self.close_reasoning(events);
            let sanitized = sanitize_tool_call_id(call.id.as_deref().unwrap_or_default());
            let id = if call.id.as_deref().is_some_and(|id| !id.is_empty()) {
                StreamPartId::wire(sanitized)
            } else {
                let index = self.next_minted_tool;
                self.next_minted_tool += 1;
                StreamPartId::minted(MintKind::Tool, index)
            };
            events.push(Ok(RawStreamingChoice::ToolCall(
                RawStreamingToolCall::new(id, call.name, call.args).with_signature(thought_signature),
            )));
            return;
        }
        let Some(text) = text else { return };
        if thought == Some(true) {
            if !self.reasoning_open {
                self.reasoning_open = true;
                events.push(Ok(RawStreamingChoice::ReasoningStart {
                    id: REASONING_ID,
                    provider_id: None,
                }));
            }
            if let Some(signature) = thought_signature {
                self.reasoning_signature = Some(signature);
            }
            self.reasoning_text.push_str(&text);
            events.push(Ok(RawStreamingChoice::ReasoningDelta {
                id: REASONING_ID,
                provider_id: None,
                reasoning: text,
            }));
            return;
        }
        // A trailing thoughtSignature can ride an empty non-thought part.
        if text.trim().is_empty() {
            if let Some(signature) = thought_signature {
                self.reasoning_signature = Some(signature);
            }
            return;
        }
        self.close_reasoning(events);
        events.push(Ok(RawStreamingChoice::Message(text)));
    }

    fn close_reasoning(&mut self, events: &mut SseEvents) {
        if !self.reasoning_open {
            return;
        }
        self.reasoning_open = false;
        let text = std::mem::take(&mut self.reasoning_text);
        let signature = self.reasoning_signature.take();
        let reasoning = rig::message::Reasoning {
            id: None,
            content: vec![rig::message::ReasoningContent::Text {
                text,
                signature: signature.clone(),
            }],
        };
        events.push(Ok(RawStreamingChoice::ReasoningEnd {
            id: REASONING_ID,
            reasoning: Some(reasoning),
            signature,
            wire_sent: false,
        }));
    }
}
