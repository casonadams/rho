//! Native Google Generative Language API SSE stream parser.
//!
//! Decodes `streamGenerateContent?alt=sse` chunks into canonical `ModelStreamEvent`s:
//! - Streams thinking deltas live and flushes `ModelStreamEvent::ReasoningBlock` with
//!   `thoughtSignature` so multi-turn Gemini 4 sessions preserve reasoning continuity.
//! - Attaches `thoughtSignature` to `ToolCall`s (falling back to any trailing thought signature).
//! - Maps `finishReason` accurately and surfaces `MALFORMED_FUNCTION_CALL` as a retryable error.

use crate::antigravity::request::sanitize_tool_call_id;
use crate::antigravity::stream::wire::{
    StreamCandidate, StreamChunk, StreamError, StreamFunctionCall, StreamPart, map_finish_reason, usage_from_metadata,
};
use crate::engine::metrics::types::Usage;
use crate::provider::adapter::ModelStreamEvent;
use crate::provider::sse::{SseEvents, SseLineDecoder, SseStreamParser, sse_line_feed};
use rho_harness_core::error::AppError;
use rho_harness_core::model::{Reasoning, ToolCall, ToolFunction};
use serde_json::Value;

#[derive(Default)]
pub struct GeminiSseParser {
    decoder: SseLineDecoder,
    reasoning_text: String,
    reasoning_signature: Option<String>,
    trailing_signature: Option<String>,
    latest_usage: Option<Usage>,
    next_minted_tool: u64,
}

fn format_error_message(error: &StreamError) -> String {
    match &error.message {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => "Unknown Gemini stream error".to_string(),
    }
}

fn stream_error(error: &StreamError) -> AppError {
    let message = format_error_message(error);
    match error.code {
        Some(429) => AppError::RateLimited {
            message,
            retry_after: None,
        },
        Some(408 | 500 | 502 | 503 | 504 | 529) => AppError::ProviderUnavailable {
            message,
            retry_after: None,
        },
        Some(code) => AppError::Provider(format!("Gemini stream error ({code}): {message}")),
        None => AppError::Provider(format!("Gemini stream error: {message}")),
    }
}

impl GeminiSseParser {
    pub fn new() -> Self {
        Self::default()
    }

    sse_line_feed!();

    fn flush_reasoning(&mut self, events: &mut SseEvents) {
        let text = std::mem::take(&mut self.reasoning_text);
        let sig = self.reasoning_signature.take();
        if sig.is_some() {
            self.trailing_signature = sig.clone();
        }
        if !text.is_empty() || sig.is_some() {
            events.push(Ok(ModelStreamEvent::ReasoningBlock(Reasoning::new_with_signature(
                text, sig,
            ))));
        }
    }

    fn handle_part_thought(&mut self, text: String, signature: Option<String>, events: &mut SseEvents) {
        if signature.is_some() {
            self.reasoning_signature = signature;
        }
        if !text.is_empty() {
            self.reasoning_text.push_str(&text);
            events.push(Ok(ModelStreamEvent::Reasoning(text)));
        }
    }

    fn handle_part_function_call(
        &mut self,
        call: StreamFunctionCall,
        signature: Option<String>,
        events: &mut SseEvents,
    ) {
        self.flush_reasoning(events);
        let sig = signature.or_else(|| self.trailing_signature.take());
        let id = match call.id.as_deref() {
            Some(raw) if !raw.is_empty() => sanitize_tool_call_id(raw),
            _ => {
                let idx = self.next_minted_tool;
                self.next_minted_tool += 1;
                format!("call-{idx}")
            }
        };
        let mut tool_call = ToolCall::new(id, ToolFunction::new(call.name, call.args));
        tool_call.signature = sig;
        events.push(Ok(ModelStreamEvent::ToolCall(tool_call)));
    }

    fn interpret_part(&mut self, part: StreamPart, events: &mut SseEvents) {
        let StreamPart {
            text,
            thought,
            thought_signature,
            function_call,
        } = part;
        if let Some(call) = function_call {
            self.handle_part_function_call(call, thought_signature, events);
            return;
        }
        if thought == Some(true) {
            self.handle_part_thought(text.unwrap_or_default(), thought_signature, events);
            return;
        }
        if let Some(sig) = thought_signature {
            self.trailing_signature = Some(sig);
        }
        if let Some(text) = text
            && !text.is_empty()
        {
            self.flush_reasoning(events);
            self.trailing_signature = None;
            events.push(Ok(ModelStreamEvent::Text(text)));
        }
    }

    fn handle_finish(&mut self, reason: &str, events: &mut SseEvents) {
        self.flush_reasoning(events);
        self.trailing_signature = None;
        if let Some(usage) = self.latest_usage.take() {
            events.push(Ok(ModelStreamEvent::Usage(usage.into())));
        }
        if reason == "MALFORMED_FUNCTION_CALL" {
            events.push(Err(AppError::ProviderUnavailable {
                message: "Gemini returned MALFORMED_FUNCTION_CALL".to_string(),
                retry_after: None,
            }));
            return;
        }
        events.push(Ok(ModelStreamEvent::Finish(map_finish_reason(reason))));
    }

    fn handle_candidate(&mut self, candidate: &mut StreamCandidate, events: &mut SseEvents) {
        if let Some(content) = candidate.content.take() {
            for part in content.parts {
                self.interpret_part(part, events);
            }
        }
        if let Some(reason) = candidate.finish_reason.as_deref() {
            self.handle_finish(reason, events);
        }
    }

    fn interpret_line(&mut self, json_line: &str, events: &mut SseEvents) {
        let Ok(chunk) = serde_json::from_str::<StreamChunk>(json_line) else {
            return;
        };
        if let Some(error) = &chunk.error {
            events.push(Err(stream_error(error)));
            return;
        }
        let mut body = chunk.response.unwrap_or(chunk.direct);
        if let Some(metadata) = body.usage_metadata.as_ref() {
            self.latest_usage = Some(usage_from_metadata(metadata));
        }
        if body.candidates.is_empty() {
            if let Some(usage) = self.latest_usage.take() {
                events.push(Ok(ModelStreamEvent::Usage(usage.into())));
            }
            return;
        }
        for candidate in &mut body.candidates {
            self.handle_candidate(candidate, events);
        }
    }
}

impl SseStreamParser for GeminiSseParser {
    fn feed(&mut self, bytes: &[u8]) -> SseEvents {
        GeminiSseParser::feed(self, bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::adapter::FinishReason;

    fn feed_sse(lines: &[&str]) -> SseEvents {
        let mut parser = GeminiSseParser::new();
        let mut payload = String::new();
        for line in lines {
            payload.push_str("data: ");
            payload.push_str(line);
            payload.push_str("\n\n");
        }
        parser.feed(payload.as_bytes())
    }

    #[test]
    fn streams_thoughts_emits_reasoning_block_and_attaches_signature_to_tool_call() {
        let events = feed_sse(&[
            r#"{"candidates":[{"content":{"parts":[{"text":"Step 1. ","thought":true}]}}]}"#,
            r#"{"candidates":[{"content":{"parts":[{"text":"Step 2.","thought":true,"thoughtSignature":"YWJjZA=="}]}}]}"#,
            r#"{"candidates":[{"content":{"parts":[{"functionCall":{"id":"fc-1","name":"read","args":{"path":"lib.rs"}},"thoughtSignature":"ZWZnaA=="}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":120,"candidatesTokenCount":25,"thoughtsTokenCount":15,"cachedContentTokenCount":80,"totalTokenCount":160}}"#,
        ]);

        assert_eq!(events.len(), 6);
        assert_eq!(
            events[0].as_ref().unwrap(),
            &ModelStreamEvent::Reasoning("Step 1. ".to_string())
        );
        assert_eq!(
            events[1].as_ref().unwrap(),
            &ModelStreamEvent::Reasoning("Step 2.".to_string())
        );
        assert_eq!(
            events[2].as_ref().unwrap(),
            &ModelStreamEvent::ReasoningBlock(Reasoning::new_with_signature(
                "Step 1. Step 2.",
                Some("YWJjZA==".to_string())
            ))
        );
        match events[3].as_ref().unwrap() {
            ModelStreamEvent::ToolCall(call) => {
                assert_eq!(call.id, "fc-1");
                assert_eq!(call.function.name, "read");
                assert_eq!(call.signature.as_deref(), Some("ZWZnaA=="));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
        match events[4].as_ref().unwrap() {
            ModelStreamEvent::Usage(u) => {
                assert_eq!(u.input_tokens, 40); // 120 - 80 cached
                assert_eq!(u.output_tokens, 40); // 25 + 15 thoughts
                assert_eq!(u.cached_input_tokens, Some(80));
                assert_eq!(u.reasoning_tokens, Some(15));
            }
            other => panic!("expected Usage, got {other:?}"),
        }
        assert_eq!(
            events[5].as_ref().unwrap(),
            &ModelStreamEvent::Finish(FinishReason::Stop)
        );
    }

    #[test]
    fn malformed_function_call_surfaces_as_retryable_provider_unavailable() {
        let events = feed_sse(&[r#"{"candidates":[{"finishReason":"MALFORMED_FUNCTION_CALL"}]}"#]);
        assert!(matches!(
            events.last(),
            Some(Err(AppError::ProviderUnavailable { message, .. })) if message.contains("MALFORMED_FUNCTION_CALL")
        ));
    }

    #[test]
    fn maps_max_tokens_finish_reason() {
        let events =
            feed_sse(&[r#"{"candidates":[{"content":{"parts":[{"text":"hi"}]},"finishReason":"MAX_TOKENS"}]}"#]);
        assert_eq!(
            events.last().unwrap().as_ref().unwrap(),
            &ModelStreamEvent::Finish(FinishReason::Length)
        );
    }
}
