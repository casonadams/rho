use crate::engine::metrics::{RunTracker, ToolOutcome};
use rho_harness_core::presentation::ActivityToken;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Instant;

#[derive(Clone)]
pub struct CompletedTool {
    pub internal_call_id: String,
    pub name: String,
    pub arguments: Value,
    pub output: String,
    pub status: ToolOutcome,
}

pub struct TurnArtifacts {
    pub response: crate::engine::metrics::types::PromptResponse,
    pub tool_calls_count: usize,
    pub completed_tools: Vec<CompletedTool>,
    pub generation_elapsed_ms: u64,
    pub turn_started_at: Instant,
    pub ttft_ms: Option<u64>,
    pub cache_miss_tokens: Option<Option<u64>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplayKind {
    #[default]
    None,
    Thinking,
    Text,
    Tool,
}

pub struct PendingToolCall {
    pub name: String,
    pub arguments: Value,
    pub started: Option<Instant>,
}

pub struct TerminalSinkConfig {
    pub model_label: String,
    pub run_tracker: RunTracker,
}

pub struct TerminalSinkState {
    pub spinner: Option<ActivityToken>,
    pub pending: HashMap<String, std::collections::VecDeque<PendingToolCall>>,
    pub reasoning: Vec<String>,
    pub completed: Vec<CompletedTool>,
    pub recorded_events: usize,
    pub last_display: DisplayKind,
    pub pending_reasoning_newlines: usize,
    pub has_reasoning_content: bool,
    pub text_carry: String,
    pub reasoning_carry: String,
    /// The secret-redaction carry of a tool card shown while the model still writes the call's arguments.
    pub tool_preview: Option<String>,
    /// When the first preview fragment of the call arrived, so its duration includes the model writing it.
    pub tool_preview_started: Option<Instant>,
    /// The secret-redaction carry of the output a running tool streams to its card.
    pub tool_output_carry: String,
}

pub struct ToolFinishDetails<'a> {
    pub name: &'a str,
    pub arguments: &'a Value,
    pub output: &'a str,
    pub is_error: bool,
}
