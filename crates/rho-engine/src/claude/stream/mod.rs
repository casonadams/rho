//! Anthropic SSE stream event decoding into rig's streaming representations.

mod wire;

#[cfg(test)]
mod tests;

pub use wire::map_finish_reason;
use wire::{ContentBlockStartPayload, ContentDeltaPayload, SseMessage};

use rig::completion::{CompletionError, FinishReason, Usage};
use rig::streaming::{MintKind, RawStreamingChoice, RawStreamingToolCall, StreamFinal, StreamPartId};
use std::collections::HashMap;

pub type SseEvents = Vec<Result<RawStreamingChoice<StreamFinal>, CompletionError>>;

#[derive(Default)]
pub struct SseParser {
    buffer: Vec<u8>,
    input_tokens: u64,
    output_tokens: u64,
    finish_reason: Option<FinishReason>,
    thinking_open: bool,
    thinking_text: String,
    thinking_signature: Option<String>,
    tool_uses: HashMap<usize, ToolUseState>,
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

    pub fn feed(&mut self, bytes: &[u8]) -> SseEvents {
        self.buffer.extend_from_slice(bytes);
        let mut events = Vec::new();
        while let Some(pos) = self.buffer.iter().position(|&b| b == b'\n') {
            let line_bytes = self.buffer[..pos].to_vec();
            self.buffer.drain(..=pos);
            let line = String::from_utf8_lossy(&line_bytes);
            self.interpret_line(line.trim_end_matches('\r'), &mut events);
        }
        events
    }

    fn interpret_line(&mut self, line: &str, events: &mut SseEvents) {
        let Some(data) = line.strip_prefix("data:") else {
            return;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            return;
        }
        let Ok(msg) = serde_json::from_str::<SseMessage>(data) else {
            return;
        };
        self.interpret_message(msg, events);
    }

    fn interpret_message(&mut self, msg: SseMessage, events: &mut SseEvents) {
        match msg {
            SseMessage::MessageStart { message } => {
                if let Some(usage) = message.usage {
                    self.input_tokens = usage.input_tokens;
                }
            }
            SseMessage::ContentBlockStart { index, content_block } => match content_block {
                ContentBlockStartPayload::Thinking { signature } => {
                    self.thinking_open = true;
                    self.thinking_text.clear();
                    self.thinking_signature = signature;
                    events.push(Ok(RawStreamingChoice::ReasoningStart {
                        id: StreamPartId::minted(MintKind::Reasoning, index as u64),
                        provider_id: None,
                    }));
                }
                ContentBlockStartPayload::ToolUse { id, name } => {
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
            },
            SseMessage::ContentBlockDelta { index, delta } => match delta {
                ContentDeltaPayload::TextDelta { text } => {
                    events.push(Ok(RawStreamingChoice::Message(text)));
                }
                ContentDeltaPayload::ThinkingDelta { thinking } => {
                    self.thinking_text.push_str(&thinking);
                    events.push(Ok(RawStreamingChoice::ReasoningDelta {
                        id: StreamPartId::minted(MintKind::Reasoning, index as u64),
                        provider_id: None,
                        reasoning: thinking,
                    }));
                }
                ContentDeltaPayload::SignatureDelta { signature } => {
                    self.thinking_signature
                        .get_or_insert_with(String::new)
                        .push_str(&signature);
                }
                ContentDeltaPayload::InputJsonDelta { partial_json } => {
                    if let Some(tool) = self.tool_uses.get_mut(&index) {
                        tool.input_json.push_str(&partial_json);
                    }
                }
                ContentDeltaPayload::Other => {}
            },
            SseMessage::ContentBlockStop { index } => self.handle_block_stop(index, events),
            SseMessage::MessageDelta { delta, usage } => {
                if let Some(reason) = delta.stop_reason {
                    self.finish_reason = Some(map_finish_reason(&reason));
                }
                if let Some(usage) = usage {
                    self.output_tokens = usage.output_tokens;
                }
            }
            SseMessage::MessageStop => {
                let mut usage = Usage::new();
                usage.input_tokens = self.input_tokens;
                usage.output_tokens = self.output_tokens;
                usage.total_tokens = self.input_tokens + self.output_tokens;
                let finish = self.finish_reason.take().unwrap_or(FinishReason::Stop);
                let final_resp = StreamFinal::new("claude", usage).with_finish_reason(finish);
                events.push(Ok(RawStreamingChoice::FinalResponse(final_resp)));
            }
            SseMessage::Error { error } => {
                let msg = error.message.unwrap_or_else(|| "Anthropic streaming error".to_string());
                events.push(Err(CompletionError::ProviderError(msg)));
            }
            SseMessage::Ignored => {}
        }
    }

    fn handle_block_stop(&mut self, index: usize, events: &mut SseEvents) {
        if self.thinking_open {
            self.thinking_open = false;
            let text = std::mem::take(&mut self.thinking_text);
            let signature = self.thinking_signature.take();
            let reasoning = rig::message::Reasoning {
                id: None,
                content: vec![rig::message::ReasoningContent::Text {
                    text,
                    signature: signature.clone(),
                }],
            };
            events.push(Ok(RawStreamingChoice::ReasoningEnd {
                id: StreamPartId::minted(MintKind::Reasoning, index as u64),
                reasoning: Some(reasoning),
                signature,
                wire_sent: true,
            }));
        } else if let Some(tool) = self.tool_uses.remove(&index) {
            let args = if tool.input_json.trim().is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(&tool.input_json).unwrap_or_else(|_| serde_json::json!({}))
            };
            events.push(Ok(RawStreamingChoice::ToolCall(RawStreamingToolCall::new(
                StreamPartId::wire(tool.id),
                tool.name,
                args,
            ))));
        }
    }
}
