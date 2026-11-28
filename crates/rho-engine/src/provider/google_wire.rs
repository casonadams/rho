//! Google Generative Language wire format and the SSE parser shared by Gemini and Antigravity.
//!
//! `StreamChunk` accepts both the bare Gemini body and Antigravity's `{"response": ...}` envelope;
//! `ThoughtReplay` is the one behavior that differs between the two.

use crate::antigravity::request::sanitize_tool_call_id;
use crate::engine::metrics::types::Usage;
use crate::provider::adapter::{FinishReason, ModelStreamEvent};
use crate::provider::error::{HttpFailure, classify_stream_error};
use crate::provider::sse::{SseEvents, SseLineDecoder, SseStreamParser, sse_line_feed};
use rho_harness_core::error::AppError;
use rho_harness_core::model::{Reasoning, ToolCall, ToolFunction};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StreamPart {
    #[serde(default)]
    pub(crate) text: Option<String>,
    #[serde(default)]
    pub(crate) thought: Option<bool>,
    #[serde(default)]
    pub(crate) thought_signature: Option<String>,
    #[serde(default)]
    pub(crate) function_call: Option<StreamFunctionCall>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StreamFunctionCall {
    #[serde(default)]
    pub(crate) id: Option<String>,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) args: Value,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StreamCandidate {
    #[serde(default)]
    pub(crate) content: Option<StreamContent>,
    #[serde(default)]
    pub(crate) finish_reason: Option<String>,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StreamContent {
    #[serde(default)]
    pub(crate) parts: Vec<StreamPart>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageMetadata {
    #[serde(default)]
    pub(crate) prompt_token_count: u64,
    #[serde(default)]
    pub(crate) candidates_token_count: u64,
    #[serde(default)]
    pub(crate) thoughts_token_count: u64,
    #[serde(default)]
    pub(crate) cached_content_token_count: u64,
    #[serde(default)]
    pub(crate) total_token_count: u64,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StreamChunk {
    #[serde(default)]
    pub(crate) response: Option<StreamResponseBody>,
    #[serde(flatten)]
    pub(crate) direct: StreamResponseBody,
    #[serde(default)]
    pub(crate) error: Option<StreamError>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StreamResponseBody {
    #[serde(default)]
    pub(crate) candidates: Vec<StreamCandidate>,
    #[serde(default)]
    pub(crate) usage_metadata: Option<UsageMetadata>,
}

#[derive(Deserialize, Debug)]
pub(crate) struct StreamError {
    #[serde(default)]
    pub(crate) code: Option<u16>,
    pub(crate) message: Option<Value>,
}

impl StreamError {
    pub(crate) fn message_text(&self, fallback: &str) -> String {
        match &self.message {
            Some(Value::String(text)) => text.clone(),
            Some(other) => other.to_string(),
            None => fallback.to_string(),
        }
    }
}

#[derive(Default)]
pub(crate) struct ToolCallIds {
    next_minted: u64,
}

impl ToolCallIds {
    pub(crate) fn resolve(&mut self, provided: Option<&str>) -> String {
        match provided.filter(|id| !id.is_empty()) {
            Some(id) => sanitize_tool_call_id(id),
            None => {
                let index = self.next_minted;
                self.next_minted += 1;
                format!("call-{index}")
            }
        }
    }
}

pub(crate) fn usage_from_metadata(metadata: &UsageMetadata) -> Usage {
    let mut usage = Usage::new();
    usage.input_tokens = Some(
        metadata
            .prompt_token_count
            .saturating_sub(metadata.cached_content_token_count),
    );
    usage.output_tokens = Some(metadata.candidates_token_count + metadata.thoughts_token_count);
    usage.total_tokens = Some(metadata.total_token_count);
    usage.cached_input_tokens = Some(metadata.cached_content_token_count);
    usage.reasoning_tokens = Some(metadata.thoughts_token_count);
    usage
}

pub fn map_finish_reason(reason: &str) -> FinishReason {
    match reason {
        "STOP" => FinishReason::Stop,
        "MAX_TOKENS" => FinishReason::Length,
        "SAFETY" | "PROHIBITED_CONTENT" | "BLOCKLIST" => FinishReason::ContentFilter,
        _ => FinishReason::Other,
    }
}

fn stream_error(error: &StreamError) -> AppError {
    let message = error.message_text("unknown provider error");
    match error.code {
        Some(status) => HttpFailure::status(status, message).into_app_error(|_, body| body.to_string()),
        None => classify_stream_error(None, message),
    }
}

/// How a Google dialect hands streamed thoughts back to the caller.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ThoughtReplay {
    /// Emit a `ReasoningBlock` with the thought signature after each run of thoughts.
    #[default]
    ReasoningBlocks,
    /// Only carry the thought signature onto the next tool call.
    ToolCallSignature,
}

/// Incremental parser for `streamGenerateContent?alt=sse`.
#[derive(Default)]
pub struct GoogleSseParser {
    decoder: SseLineDecoder,
    thoughts: ThoughtReplay,
    reasoning_text: String,
    reasoning_signature: Option<String>,
    trailing_signature: Option<String>,
    latest_usage: Option<Usage>,
    usage_reported: bool,
    finish_seen: bool,
    failed: bool,
    tool_ids: ToolCallIds,
}

impl GoogleSseParser {
    pub(crate) fn new(thoughts: ThoughtReplay) -> Self {
        Self {
            thoughts,
            ..Self::default()
        }
    }

    sse_line_feed!();

    fn flush_reasoning(&mut self, events: &mut SseEvents) {
        let text = std::mem::take(&mut self.reasoning_text);
        let sig = self.reasoning_signature.take();
        if sig.is_some() {
            self.trailing_signature = sig.clone();
        }
        if self.thoughts == ThoughtReplay::ReasoningBlocks && (!text.is_empty() || sig.is_some()) {
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
        let id = self.tool_ids.resolve(call.id.as_deref());
        let mut tool_call = ToolCall::new(id, ToolFunction::new(call.name, call.args));
        tool_call.signature = sig;
        events.push(Ok(ModelStreamEvent::ToolCall(tool_call)));
    }

    fn handle_part_text(&mut self, text: String, signature: Option<String>, events: &mut SseEvents) {
        if let Some(sig) = signature
            && text.trim().is_empty()
        {
            self.trailing_signature = Some(sig);
            return;
        }
        if text.is_empty() {
            return;
        }
        self.flush_reasoning(events);
        self.trailing_signature = None;
        events.push(Ok(ModelStreamEvent::Text(text)));
    }

    fn interpret_part(&mut self, part: StreamPart, events: &mut SseEvents) {
        let StreamPart {
            text,
            thought,
            thought_signature,
            function_call,
        } = part;
        let text = text.unwrap_or_default();
        match function_call {
            Some(call) => self.handle_part_function_call(call, thought_signature, events),
            None if thought == Some(true) => self.handle_part_thought(text, thought_signature, events),
            None => self.handle_part_text(text, thought_signature, events),
        }
    }

    fn report_usage(&mut self, events: &mut SseEvents) {
        if self.usage_reported {
            return;
        }
        if let Some(usage) = self.latest_usage.take() {
            self.usage_reported = true;
            events.push(Ok(ModelStreamEvent::Usage(usage.into())));
        }
    }

    fn handle_finish(&mut self, reason: &str, events: &mut SseEvents) {
        self.flush_reasoning(events);
        self.trailing_signature = None;
        self.report_usage(events);
        self.finish_seen = true;
        if reason == "MALFORMED_FUNCTION_CALL" {
            self.failed = true;
            events.push(Err(AppError::ProviderUnavailable {
                message: "The model returned MALFORMED_FUNCTION_CALL".to_string(),
                retry_after: None,
            }));
            return;
        }
        events.push(Ok(ModelStreamEvent::Finish(map_finish_reason(reason))));
    }

    fn handle_candidate(&mut self, candidate: StreamCandidate, events: &mut SseEvents) {
        if let Some(content) = candidate.content {
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
            self.failed = true;
            events.push(Err(stream_error(error)));
            return;
        }
        let body = chunk.response.unwrap_or(chunk.direct);
        if let Some(metadata) = body.usage_metadata.as_ref() {
            self.latest_usage = Some(usage_from_metadata(metadata));
        }
        if body.candidates.is_empty() && self.finish_seen {
            self.report_usage(events);
        }
        for candidate in body.candidates {
            self.handle_candidate(candidate, events);
        }
    }
}

impl SseStreamParser for GoogleSseParser {
    fn feed(&mut self, bytes: &[u8]) -> SseEvents {
        GoogleSseParser::feed(self, bytes)
    }

    /// Usage metadata is cumulative and may trail the finishing candidate, so only the latest is reported, once.
    fn is_done(&self) -> bool {
        self.failed || (self.finish_seen && self.usage_reported)
    }

    fn finish(&mut self) -> SseEvents {
        if self.failed || self.finish_seen {
            return Vec::new();
        }
        vec![Err(AppError::Network(
            "Google stream closed before a finishReason".to_string(),
        ))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_finish_reason() {
        assert!(matches!(map_finish_reason("STOP"), FinishReason::Stop));
        assert!(matches!(map_finish_reason("MAX_TOKENS"), FinishReason::Length));
        assert!(matches!(map_finish_reason("SAFETY"), FinishReason::ContentFilter));
        assert!(matches!(
            map_finish_reason("PROHIBITED_CONTENT"),
            FinishReason::ContentFilter
        ));
        assert!(matches!(map_finish_reason("BLOCKLIST"), FinishReason::ContentFilter));
        assert!(matches!(map_finish_reason("UNKNOWN"), FinishReason::Other));
    }

    #[test]
    fn stream_error_text_uses_string_other_or_fallback() {
        let text = |message| StreamError { code: None, message }.message_text("fallback");
        assert_eq!(text(Some(Value::String("boom".into()))), "boom");
        assert_eq!(text(Some(serde_json::json!({ "k": 1 }))), r#"{"k":1}"#);
        assert_eq!(text(None), "fallback");
    }

    #[test]
    fn tool_call_ids_keep_provider_ids_and_mint_missing_ones_in_order() {
        let mut ids = ToolCallIds::default();
        assert_eq!(ids.resolve(Some("fc-1")), "fc-1");
        assert_eq!(ids.resolve(Some("")), "call-0");
        assert_eq!(ids.resolve(None), "call-1");
    }
}
