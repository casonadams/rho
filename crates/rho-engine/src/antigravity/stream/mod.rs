//! Antigravity wire format, stream side: SSE chunk decoding into
//! canonical streaming events.

pub mod wire;

#[cfg(test)]
mod tests;

pub use wire::map_finish_reason;
use wire::{StreamCandidate, StreamChunk, StreamFunctionCall, StreamPart, usage_from_metadata};

use crate::provider::adapter::{FinishReason, ModelStreamEvent};
use crate::provider::sse::SseLineDecoder;
use futures::StreamExt;
use rho_harness_core::error::AppError;
use rho_harness_core::model::{ToolCall, ToolFunction};
use serde_json::Value;

use super::request::sanitize_tool_call_id;

/// Incremental SSE parser for Antigravity `streamGenerateContent?alt=sse`.
pub struct SseParser {
    decoder: SseLineDecoder,
    reasoning_open: bool,
    reasoning_text: String,
    reasoning_signature: Option<String>,
    next_minted_tool: u64,
}

pub type SseEvents = Vec<Result<ModelStreamEvent, AppError>>;

impl Default for SseParser {
    fn default() -> Self {
        Self::new()
    }
}

fn format_chunk_error(error: &wire::StreamError) -> String {
    match &error.message {
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
        None => "unknown provider error".to_string(),
    }
}

impl SseParser {
    pub fn new() -> Self {
        Self {
            decoder: SseLineDecoder::new(),
            reasoning_open: false,
            reasoning_text: String::new(),
            reasoning_signature: None,
            next_minted_tool: 0,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) -> SseEvents {
        let mut events = Vec::new();
        for line in self.decoder.decode_lines(bytes) {
            self.interpret_line(&line, &mut events);
        }
        events
    }

    fn handle_candidate(
        &mut self,
        candidate: &mut StreamCandidate,
        usage: crate::engine::metrics::types::Usage,
        events: &mut SseEvents,
    ) {
        if let Some(content) = candidate.content.take() {
            for part in content.parts {
                self.interpret_part(part, events);
            }
        }
        if candidate.finish_reason.is_some() {
            self.close_reasoning();
            events.push(Ok(ModelStreamEvent::Usage(usage.into())));
            events.push(Ok(ModelStreamEvent::Finish(FinishReason::Stop)));
        }
    }

    fn interpret_line(&mut self, json_line: &str, events: &mut SseEvents) {
        let Ok(chunk) = serde_json::from_str::<StreamChunk>(json_line) else {
            return;
        };
        if let Some(ref error) = chunk.error {
            events.push(Err(AppError::Provider(format_chunk_error(error))));
            return;
        }
        let mut body = chunk.response.unwrap_or(chunk.direct);
        let usage = body
            .usage_metadata
            .as_ref()
            .map(usage_from_metadata)
            .unwrap_or_default();
        for candidate in &mut body.candidates {
            self.handle_candidate(candidate, usage, events);
        }
    }

    fn handle_part_function_call(
        &mut self,
        (call, signature): (StreamFunctionCall, Option<String>),
        events: &mut SseEvents,
    ) {
        let sig = signature.or_else(|| self.reasoning_signature.take());
        self.close_reasoning();
        let sanitized = sanitize_tool_call_id(call.id.as_deref().unwrap_or_default());
        let id = if call.id.as_deref().is_some_and(|id| !id.is_empty()) {
            sanitized
        } else {
            let index = self.next_minted_tool;
            self.next_minted_tool += 1;
            format!("call-{index}")
        };
        let mut tool_call = ToolCall::new(id, ToolFunction::new(call.name, call.args));
        tool_call.signature = sig;
        events.push(Ok(ModelStreamEvent::ToolCall(tool_call)));
    }

    fn handle_part_thought(&mut self, text: String, signature: Option<String>, events: &mut SseEvents) {
        self.reasoning_open = true;
        if let Some(sig) = &signature {
            self.reasoning_signature = Some(sig.clone());
        }
        let _sig = signature.or_else(|| self.reasoning_signature.clone());
        self.reasoning_text.push_str(&text);
        events.push(Ok(ModelStreamEvent::Reasoning(text)));
    }

    fn interpret_part(&mut self, part: StreamPart, events: &mut SseEvents) {
        let StreamPart {
            text,
            thought,
            thought_signature,
            function_call,
        } = part;
        if let Some(call) = function_call {
            self.handle_part_function_call((call, thought_signature), events);
            return;
        }
        let Some(text) = text else { return };
        if thought == Some(true) {
            self.handle_part_thought(text, thought_signature, events);
        } else if text.trim().is_empty() {
            if let Some(signature) = thought_signature {
                self.reasoning_signature = Some(signature);
            }
        } else {
            self.close_reasoning();
            events.push(Ok(ModelStreamEvent::Text(text)));
        }
    }

    fn close_reasoning(&mut self) {
        self.reasoning_open = false;
        self.reasoning_text.clear();
        self.reasoning_signature = None;
    }
}

pub fn unfold_antigravity_stream(
    response: reqwest::Response,
) -> impl futures::Stream<Item = Result<ModelStreamEvent, AppError>> + Send + 'static {
    futures::stream::unfold((response.bytes_stream(), SseParser::new(), false), next_stream_batch)
        .map(futures::stream::iter)
        .flatten()
}

type StreamState<S> = (S, SseParser, bool);

async fn next_stream_batch<B, S>(
    (mut byte_stream, mut parser, finished): StreamState<S>,
) -> Option<(Vec<Result<ModelStreamEvent, AppError>>, StreamState<S>)>
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
                        .any(|e| matches!(e, Ok(ModelStreamEvent::Finish(_)) | Err(_)));
                    return Some((events, (byte_stream, parser, has_term)));
                }
            }
            Err(e) => {
                let err = AppError::Provider(format!("Antigravity stream transport failed: {e}"));
                return Some((vec![Err(err)], (byte_stream, parser, true)));
            }
        }
    }
    None
}
