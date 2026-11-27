//! Google Generative Language wire format shared by the Gemini and Antigravity SSE parsers.

use crate::antigravity::request::sanitize_tool_call_id;
use crate::engine::metrics::types::Usage;
use crate::provider::adapter::FinishReason;
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
