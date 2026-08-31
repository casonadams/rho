use rig::agent::{CompletionCall, PromptResponse};
use rig::completion::{FinishReason, Usage};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalStatus {
    Completed,
    ContentFiltered,
    BudgetExhausted,
    Cancelled,
    Failed,
}

impl TerminalStatus {
    pub fn is_success(self) -> bool {
        matches!(self, Self::Completed | Self::ContentFiltered)
    }
}

pub fn format_tokens(count: u64) -> String {
    if count < 1_000 {
        count.to_string()
    } else if count < 100_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else if count < 1_000_000 {
        format!("{}k", (count as f64 / 1_000.0).round() as u64)
    } else if count.is_multiple_of(1_000_000) {
        format!("{}M", count / 1_000_000)
    } else {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuralUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_prompt_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
}

impl StructuralUsage {
    pub fn has_values(self) -> bool {
        self.input_tokens != 0 || self.output_tokens != 0 || self.total_tokens != 0
    }
}

impl From<Usage> for StructuralUsage {
    fn from(usage: Usage) -> Self {
        Self {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: usage.total_tokens,
            cached_input_tokens: nonzero(usage.cached_input_tokens),
            cache_creation_input_tokens: nonzero(usage.cache_creation_input_tokens),
            tool_use_prompt_tokens: nonzero(usage.tool_use_prompt_tokens),
            reasoning_tokens: nonzero(usage.reasoning_tokens),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCallMetrics {
    pub call_index: usize,
    pub usage_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<StructuralUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_status: Option<String>,
}

impl From<&CompletionCall> for ModelCallMetrics {
    fn from(call: &CompletionCall) -> Self {
        Self {
            call_index: call.call_index,
            usage_available: call.usage.has_values(),
            usage: call.usage.has_values().then(|| call.usage.into()),
            finish_status: call.finish_reason.as_ref().map(finish_status),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunMetrics {
    pub session_id: String,
    pub success: bool,
    pub terminal_status: TerminalStatus,
    pub elapsed_ms: u64,
    pub model_turns: usize,
    pub requests: usize,
    pub tool_calls: usize,
    pub tool_errors: usize,
    pub tool_denials: usize,
    pub usage_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<StructuralUsage>,
    pub completion_calls: Vec<ModelCallMetrics>,
}

impl RunMetrics {
    pub fn normalized(mut self) -> Self {
        self.session_id = "<session>".to_string();
        self.elapsed_ms = 0;
        self
    }
}

pub struct NeutralOutcome<'a> {
    pub session_id: &'a str,
    pub status: TerminalStatus,
    pub requests: usize,
    pub usage: Option<StructuralUsage>,
}

#[derive(Debug)]
pub struct CompletionOutcome<'a> {
    pub session_id: &'a str,
    pub status: TerminalStatus,
    pub response: &'a PromptResponse,
}

pub(crate) struct ObservationMetricsInput<'a> {
    pub(crate) observation: Option<super::tracker::RunObservation>,
    pub(crate) session_id: &'a str,
    pub(crate) status: TerminalStatus,
    pub(crate) usage: Option<StructuralUsage>,
    pub(crate) completion_calls: Vec<ModelCallMetrics>,
    pub(crate) requests: usize,
}

pub(crate) fn metrics_from_observation(input: ObservationMetricsInput<'_>) -> RunMetrics {
    let (elapsed_ms, tool_calls, tool_errors, tool_denials) = input.observation.map_or((0, 0, 0, 0), |state| {
        (
            state.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            state.tool_calls,
            state.tool_errors,
            state.tool_denials,
        )
    });
    RunMetrics {
        session_id: input.session_id.to_string(),
        success: input.status.is_success(),
        terminal_status: input.status,
        elapsed_ms,
        model_turns: input.requests,
        requests: input.requests,
        tool_calls,
        tool_errors,
        tool_denials,
        usage_available: input.usage.is_some(),
        usage: input.usage,
        completion_calls: input.completion_calls,
    }
}

pub(crate) fn finish_status(reason: &FinishReason) -> String {
    match reason {
        FinishReason::Stop => "stop".to_string(),
        FinishReason::Length => "length".to_string(),
        FinishReason::ToolCalls => "tool_calls".to_string(),
        FinishReason::ContentFilter => "content_filter".to_string(),
        FinishReason::Other(_) => "other".to_string(),
    }
}

pub(crate) fn nonzero(value: u64) -> Option<u64> {
    (value != 0).then_some(value)
}
