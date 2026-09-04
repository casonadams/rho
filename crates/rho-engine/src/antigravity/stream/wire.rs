//! Antigravity wire format definitions and JSON deserialization schemas.

use rig::completion::{FinishReason, Usage};
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
    pub(crate) message: Option<Value>,
}

pub(crate) fn usage_from_metadata(metadata: &UsageMetadata) -> Usage {
    Usage {
        input_tokens: metadata
            .prompt_token_count
            .saturating_sub(metadata.cached_content_token_count),
        output_tokens: metadata.candidates_token_count + metadata.thoughts_token_count,
        total_tokens: metadata.total_token_count,
        cached_input_tokens: metadata.cached_content_token_count,
        cache_creation_input_tokens: 0,
        tool_use_prompt_tokens: 0,
        reasoning_tokens: metadata.thoughts_token_count,
    }
}

pub fn map_finish_reason(reason: &str) -> FinishReason {
    match reason {
        "STOP" => FinishReason::Stop,
        "MAX_TOKENS" => FinishReason::Length,
        "SAFETY" | "PROHIBITED_CONTENT" | "BLOCKLIST" => FinishReason::ContentFilter,
        other => FinishReason::Other(other.to_string()),
    }
}
