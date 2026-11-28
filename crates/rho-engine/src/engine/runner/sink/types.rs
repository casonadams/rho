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
    /// The provider refused the turn's content with an error rather than a finish reason.
    pub content_filtered: bool,
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
    pub tool_preview: Option<ToolPreview>,
    /// The secret-redaction carry of the output a running tool streams to its card.
    pub tool_output_carry: String,
}

/// The card of a tool call shown while the model still writes its arguments.
pub struct ToolPreview {
    pub call_id: String,
    /// The secret-redaction carry of the content streamed to the card.
    pub carry: String,
    /// When the call's first fragment arrived, so its duration includes the model writing it.
    pub started: Instant,
}

impl ToolPreview {
    /// The preview of `call_id` with a fresh carry, timed from `previous` when that shows the same call.
    pub fn for_call(call_id: &str, previous: Option<Self>) -> Self {
        let started = previous
            .filter(|preview| preview.call_id == call_id)
            .map_or_else(Instant::now, |preview| preview.started);
        Self {
            call_id: call_id.to_string(),
            carry: String::new(),
            started,
        }
    }
}

pub struct ToolFinishDetails<'a> {
    pub name: &'a str,
    pub arguments: &'a Value,
    pub output: &'a str,
    pub is_error: bool,
}

#[cfg(test)]
mod tests {
    use super::ToolPreview;
    use std::time::{Duration, Instant};

    #[test]
    fn a_preview_keeps_its_start_only_for_the_same_call() {
        let started = Instant::now().checked_sub(Duration::from_secs(5)).unwrap();
        let previous = || ToolPreview {
            call_id: "a".into(),
            carry: "partial".into(),
            started,
        };

        let same = ToolPreview::for_call("a", Some(previous()));
        assert_eq!(same.started, started);
        assert!(same.carry.is_empty());

        let other = ToolPreview::for_call("b", Some(previous()));
        assert_eq!(other.call_id, "b");
        assert!(other.started > started);
    }
}
