//! Long-context comparison harness: bounded vs. unbounded history.

use crate::engine::metrics::TerminalStatus;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextComparisonReport {
    pub scenario: &'static str,
    pub before: ContextEvaluation,
    pub after: ContextEvaluation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextEvaluation {
    pub model_visible_messages: usize,
    pub model_visible_bytes: usize,
    pub input_tokens: Option<u64>,
    pub success: bool,
    pub terminal_status: TerminalStatus,
    pub turns: usize,
    pub tool_calls: usize,
    pub tool_errors: usize,
    pub tool_denials: usize,
    pub usage_available: bool,
}
