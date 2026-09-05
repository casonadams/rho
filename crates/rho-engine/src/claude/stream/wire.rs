//! Wire types for Anthropic SSE streaming events.

use rig::completion::FinishReason;
use serde::Deserialize;

pub fn map_finish_reason(reason: &str) -> FinishReason {
    match reason {
        "end_turn" => FinishReason::Stop,
        "max_tokens" => FinishReason::Length,
        "stop_sequence" => FinishReason::Stop,
        "tool_use" => FinishReason::ToolCalls,
        other => FinishReason::Other(other.to_string()),
    }
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub(super) enum SseMessage {
    #[serde(rename = "message_start")]
    MessageStart { message: MessageStartPayload },
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: usize,
        content_block: ContentBlockStartPayload,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { index: usize, delta: ContentDeltaPayload },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: usize },
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: MessageDeltaPayload,
        usage: Option<MessageDeltaUsage>,
    },
    #[serde(rename = "message_stop")]
    MessageStop,
    #[serde(rename = "error")]
    Error { error: ErrorPayload },
    #[serde(other)]
    Ignored,
}

#[derive(Deserialize)]
pub(super) struct MessageStartPayload {
    pub usage: Option<MessageStartUsage>,
}

#[derive(Deserialize)]
pub(super) struct MessageStartUsage {
    #[serde(default)]
    pub input_tokens: u64,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub(super) enum ContentBlockStartPayload {
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "thinking")]
    Thinking {
        #[serde(default)]
        signature: Option<String>,
    },
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub(super) enum ContentDeltaPayload {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { thinking: String },
    #[serde(rename = "signature_delta")]
    SignatureDelta { signature: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
pub(super) struct MessageDeltaPayload {
    pub stop_reason: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct MessageDeltaUsage {
    #[serde(default)]
    pub output_tokens: u64,
}

#[derive(Deserialize)]
pub(super) struct ErrorPayload {
    pub message: Option<String>,
}
