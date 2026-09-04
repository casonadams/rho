use crate::engine::metrics::RunTracker;
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
    pub status: String,
}

pub struct TurnArtifacts {
    pub response: rig::agent::PromptResponse,
    pub tool_calls_count: usize,
    pub completed_tools: Vec<CompletedTool>,
    pub generation_elapsed_ms: u64,
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
    pub auto_approve: bool,
    pub run_tracker: RunTracker,
}

pub struct TerminalSinkState {
    pub auto_approve: bool,
    pub spinner: Option<ActivityToken>,
    pub pending: HashMap<String, PendingToolCall>,
    pub reasoning: Vec<String>,
    pub completed: Vec<CompletedTool>,
    pub last_display: DisplayKind,
    pub pending_reasoning_newlines: usize,
    pub has_reasoning_content: bool,
}

pub struct ToolFinishDetails<'a> {
    pub name: &'a str,
    pub arguments: &'a Value,
    pub output: &'a str,
    pub is_error: bool,
}
