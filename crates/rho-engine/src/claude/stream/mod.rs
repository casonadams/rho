//! Anthropic SSE stream event decoding into rig's streaming representations.

mod wire;

#[cfg(test)]
mod tests;

pub use wire::map_finish_reason;
use wire::{ContentBlockStartPayload, ContentDeltaPayload, SseMessage};

use crate::adapter::rig::model::AdapterFrame;
use crate::provider::sse::SseLineDecoder;
use futures::StreamExt;
use rig::completion::Usage;
use rig::error::ProviderError;
use std::collections::HashMap;

pub type SseEvents = Vec<Result<AdapterFrame, ProviderError>>;

#[derive(Default)]
pub struct SseParser {
    decoder: SseLineDecoder,
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_input_tokens: u64,
    cache_read_input_tokens: u64,
    reasoning_tokens: u64,
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
        let lines = self.decoder.decode_lines(bytes);
        let mut events = Vec::new();
        for line in lines {
            self.interpret_line(&line, &mut events);
        }
        events
    }

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
        _events: &mut SseEvents,
    ) {
        match content_block {
            ContentBlockStartPayload::Thinking { signature } => {
                self.thinking_open = true;
                self.thinking_text.clear();
                self.thinking_signature = signature;
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
        }
    }

    fn handle_content_block_delta(&mut self, index: usize, delta: ContentDeltaPayload, events: &mut SseEvents) {
        match delta {
            ContentDeltaPayload::TextDelta { text } => {
                events.push(Ok(AdapterFrame::Text(text)));
            }
            ContentDeltaPayload::ThinkingDelta { thinking } => {
                self.thinking_text.push_str(&thinking);
                events.push(Ok(AdapterFrame::Reasoning(thinking)));
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
        }
    }

    fn handle_message_stop(&mut self, events: &mut SseEvents) {
        let usage = Usage {
            input_tokens: Some(self.input_tokens),
            output_tokens: Some(self.output_tokens),
            cached_input_tokens: Some(self.cache_read_input_tokens),
            cache_creation_input_tokens: Some(self.cache_creation_input_tokens),
            reasoning_tokens: Some(self.reasoning_tokens),
            total_tokens: Some(
                self.input_tokens
                    + self.cache_read_input_tokens
                    + self.cache_creation_input_tokens
                    + self.output_tokens,
            ),
            ..Default::default()
        };
        events.push(Ok(AdapterFrame::Done { usage }));
    }

    fn handle_message_delta(&mut self, _delta: wire::MessageDeltaPayload, usage: Option<wire::MessageDeltaUsage>) {
        if let Some(usage) = usage {
            self.output_tokens = usage.output_tokens;
            if let Some(inp) = usage.input_tokens {
                self.input_tokens = inp;
            }
            if let Some(cr) = usage.cache_read_input_tokens {
                self.cache_read_input_tokens = cr;
            }
            if let Some(cw) = usage.cache_creation_input_tokens {
                self.cache_creation_input_tokens = cw;
            }
            if let Some(details) = usage.output_tokens_details {
                self.reasoning_tokens = details.thinking_tokens;
            }
        }
    }

    fn interpret_message(&mut self, msg: SseMessage, events: &mut SseEvents) {
        match msg {
            SseMessage::MessageStart { message } => {
                if let Some(usage) = message.usage {
                    self.input_tokens = usage.input_tokens;
                    if let Some(cr) = usage.cache_read_input_tokens {
                        self.cache_read_input_tokens = cr;
                    }
                    if let Some(cw) = usage.cache_creation_input_tokens {
                        self.cache_creation_input_tokens = cw;
                    }
                    if let Some(details) = usage.output_tokens_details {
                        self.reasoning_tokens = details.thinking_tokens;
                    }
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
                events.push(Err(ProviderError::Provider(msg)));
            }
            SseMessage::Ignored => {}
        }
    }

    fn close_thinking_block(&mut self, _index: usize, _events: &mut SseEvents) {
        self.thinking_open = false;
    }

    fn close_tool_use_block(&mut self, index: usize, events: &mut SseEvents) {
        let Some(tool) = self.tool_uses.remove(&index) else {
            return;
        };
        let canonical_name = crate::claude::request::from_claude_tool_name(&tool.name).to_string();
        events.push(Ok(AdapterFrame::ToolCall {
            id: tool.id,
            name: canonical_name,
            arguments: if tool.input_json.trim().is_empty() {
                "{}".to_string()
            } else {
                tool.input_json
            },
        }));
    }

    fn handle_block_stop(&mut self, index: usize, events: &mut SseEvents) {
        if self.thinking_open {
            self.close_thinking_block(index, events);
        } else {
            self.close_tool_use_block(index, events);
        }
    }
}

pub fn unfold_claude_stream(
    response: reqwest::Response,
) -> impl futures::Stream<Item = Result<AdapterFrame, ProviderError>> + Send + 'static {
    futures::stream::unfold((response.bytes_stream(), SseParser::new(), false), next_stream_batch)
        .map(futures::stream::iter)
        .flatten()
}

type StreamState<S> = (S, SseParser, bool);

async fn next_stream_batch<B, S>(
    (mut byte_stream, mut parser, finished): StreamState<S>,
) -> Option<(Vec<Result<AdapterFrame, ProviderError>>, StreamState<S>)>
where
    B: AsRef<[u8]>,
    S: futures::Stream<Item = reqwest::Result<B>> + Unpin,
{
    if finished {
        return None;
    }
    while let Some(chunk) = byte_stream.next().await {
        match chunk {
            Ok(bytes) => {
                let events = parser.feed(bytes.as_ref());
                if !events.is_empty() {
                    let has_term = events
                        .iter()
                        .any(|e| matches!(e, Ok(AdapterFrame::Done { .. }) | Err(_)));
                    return Some((events, (byte_stream, parser, has_term)));
                }
            }
            Err(e) => {
                let err = ProviderError::Provider(format!("Claude stream transport failed: {e}"));
                return Some((vec![Err(err)], (byte_stream, parser, true)));
            }
        }
    }
    None
}
