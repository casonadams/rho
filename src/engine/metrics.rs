use rig::agent::{CompletionCall, PromptResponse};
use rig::completion::{FinishReason, Usage};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Instant;

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

#[derive(Clone, Default)]
pub struct RunTracker {
    state: Arc<Mutex<Option<RunObservation>>>,
}

struct RunObservation {
    started: Instant,
    tool_calls: usize,
    tool_errors: usize,
    tool_denials: usize,
    completion_calls: Vec<CompletionCall>,
}

#[derive(Debug)]
pub struct CompletionOutcome<'a> {
    pub session_id: &'a str,
    pub status: TerminalStatus,
    pub response: &'a PromptResponse,
}

struct ObservationMetricsInput<'a> {
    observation: Option<RunObservation>,
    session_id: &'a str,
    status: TerminalStatus,
    usage: Option<StructuralUsage>,
    completion_calls: Vec<ModelCallMetrics>,
    requests: usize,
}

fn metrics_from_observation(input: ObservationMetricsInput<'_>) -> RunMetrics {
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

impl RunTracker {
    pub fn start(&self) {
        if let Ok(mut state) = self.state.lock() {
            *state = Some(RunObservation {
                started: Instant::now(),
                tool_calls: 0,
                tool_errors: 0,
                tool_denials: 0,
                completion_calls: Vec::new(),
            });
        }
    }

    pub fn tool_called(&self) {
        if let Some(state) = self.state().as_mut() {
            state.tool_calls += 1;
        }
    }

    pub fn tool_finished(&self, status: &str) {
        if let Some(state) = self.state().as_mut() {
            if status != "success" {
                state.tool_errors += 1;
            }
            if status == "denied" {
                state.tool_denials += 1;
            }
        }
    }

    pub fn invalid_tool(&self) {
        if let Some(state) = self.state().as_mut() {
            state.tool_calls += 1;
            state.tool_errors += 1;
        }
    }

    pub fn completion(&self, call: CompletionCall) {
        if let Some(state) = self.state().as_mut() {
            state.completion_calls.push(call);
        }
    }

    pub fn complete(&self, outcome: CompletionOutcome<'_>) -> RunMetrics {
        let observation = self.take();
        let usage = outcome
            .response
            .usage
            .has_values()
            .then(|| outcome.response.usage.into());
        let calls = outcome
            .response
            .completion_calls
            .iter()
            .map(ModelCallMetrics::from)
            .collect();
        metrics_from_observation(ObservationMetricsInput {
            observation,
            session_id: outcome.session_id,
            status: outcome.status,
            usage,
            completion_calls: calls,
            requests: outcome.response.requests(),
        })
    }

    pub fn terminate(&self, session_id: &str, status: TerminalStatus) -> RunMetrics {
        let observation = self.take();
        let usage = observation.as_ref().and_then(|state| {
            let usage = state
                .completion_calls
                .iter()
                .fold(Usage::new(), |total, call| total + call.usage);
            usage.has_values().then(|| usage.into())
        });
        let calls: Vec<ModelCallMetrics> = observation
            .as_ref()
            .map(|state| state.completion_calls.iter().map(ModelCallMetrics::from).collect())
            .unwrap_or_default();
        let requests = calls.len();
        metrics_from_observation(ObservationMetricsInput {
            observation,
            session_id,
            status,
            usage,
            completion_calls: calls,
            requests,
        })
    }

    fn state(&self) -> std::sync::MutexGuard<'_, Option<RunObservation>> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn take(&self) -> Option<RunObservation> {
        self.state().take()
    }
}

fn finish_status(reason: &FinishReason) -> String {
    match reason {
        FinishReason::Stop => "stop".to_string(),
        FinishReason::Length => "length".to_string(),
        FinishReason::ToolCalls => "tool_calls".to_string(),
        FinishReason::ContentFilter => "content_filter".to_string(),
        FinishReason::Other(_) => "other".to_string(),
    }
}

fn nonzero(value: u64) -> Option<u64> {
    (value != 0).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::agent::CompletionCall;

    fn usage() -> Usage {
        Usage {
            input_tokens: 10,
            output_tokens: 4,
            total_tokens: 14,
            cached_input_tokens: 3,
            cache_creation_input_tokens: 2,
            tool_use_prompt_tokens: 1,
            reasoning_tokens: 5,
        }
    }

    #[test]
    fn usage_records_optional_cache_and_reasoning_only_when_reported() {
        let available = StructuralUsage::from(usage());
        assert_eq!(available.cached_input_tokens, Some(3));
        assert_eq!(available.reasoning_tokens, Some(5));

        let absent = StructuralUsage::from(Usage {
            input_tokens: 2,
            output_tokens: 1,
            total_tokens: 3,
            ..Usage::new()
        });
        let encoded = serde_json::to_string(&absent).unwrap();
        assert!(!encoded.contains("cached_input_tokens"));
        assert!(!encoded.contains("reasoning_tokens"));
    }

    #[test]
    fn tracker_counts_tool_errors_and_denials_separately() {
        let tracker = RunTracker::default();
        tracker.start();
        tracker.tool_called();
        tracker.tool_finished("denied");
        tracker.tool_called();
        tracker.tool_finished("error");
        let metrics = tracker.terminate("session", TerminalStatus::Failed);

        assert_eq!(metrics.tool_calls, 2);
        assert_eq!(metrics.tool_errors, 2);
        assert_eq!(metrics.tool_denials, 1);
    }

    #[test]
    fn normalized_metrics_are_stable_across_runs() {
        let response = PromptResponse::new("not recorded", usage()).with_completion_calls(vec![
            CompletionCall::new(0, usage()).with_finish_reason(Some(FinishReason::Stop)),
        ]);
        let first = RunTracker::default();
        first.start();
        first.tool_called();
        first.tool_finished("success");
        let first = first
            .complete(CompletionOutcome {
                session_id: "random-a",
                status: TerminalStatus::Completed,
                response: &response,
            })
            .normalized();
        let second = RunTracker::default();
        second.start();
        second.tool_called();
        second.tool_finished("success");
        let second = second
            .complete(CompletionOutcome {
                session_id: "random-b",
                status: TerminalStatus::Completed,
                response: &response,
            })
            .normalized();

        assert_eq!(
            serde_json::to_vec(&first).unwrap(),
            serde_json::to_vec(&second).unwrap()
        );
    }

    #[test]
    fn structural_metrics_contain_no_response_or_identity_content() {
        let sentinel = "credential-sentinel";
        let response = PromptResponse::new(sentinel, usage()).with_completion_calls(vec![
            CompletionCall::new(0, usage())
                .with_identity(rig::completion::ResponseIdentity {
                    message_id: Some(sentinel.to_string()),
                    response_id: Some(sentinel.to_string()),
                    provider_request_id: Some(sentinel.to_string()),
                })
                .with_finish_reason(Some(FinishReason::Other(sentinel.to_string()))),
        ]);
        let tracker = RunTracker::default();
        tracker.start();
        let encoded = serde_json::to_string(&tracker.complete(CompletionOutcome {
            session_id: "safe-session",
            status: TerminalStatus::Completed,
            response: &response,
        }))
        .unwrap();

        assert!(!encoded.contains(sentinel));
        assert!(!encoded.contains("\"output\":"));
        assert!(!encoded.contains("message_id"));
    }
}
