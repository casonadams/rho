use crate::engine::metrics::RunTracker;
use serde_json::Value;
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
