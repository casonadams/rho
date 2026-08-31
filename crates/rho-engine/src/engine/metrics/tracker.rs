use super::types::{
    CompletionOutcome, ModelCallMetrics, NeutralOutcome, ObservationMetricsInput, RunMetrics, TerminalStatus,
    metrics_from_observation,
};
use rig::agent::CompletionCall;
use rig::completion::Usage;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone, Default)]
pub struct RunTracker {
    state: Arc<Mutex<Option<RunObservation>>>,
}

pub(crate) struct RunObservation {
    pub(crate) started: Instant,
    pub(crate) tool_calls: usize,
    pub(crate) tool_errors: usize,
    pub(crate) tool_denials: usize,
    pub(crate) completion_calls: Vec<CompletionCall>,
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

    pub fn complete_neutral(&self, outcome: NeutralOutcome<'_>) -> RunMetrics {
        metrics_from_observation(ObservationMetricsInput {
            observation: self.take(),
            session_id: outcome.session_id,
            status: outcome.status,
            usage: outcome.usage,
            completion_calls: Vec::new(),
            requests: outcome.requests,
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
