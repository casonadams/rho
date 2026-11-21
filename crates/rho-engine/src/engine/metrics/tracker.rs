use super::types::{
    CompletionCall, CompletionOutcome, ModelCallMetrics, ObservationMetricsInput, RunMetrics, StructuralUsage,
    TerminalStatus, metrics_from_observation,
};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone, Default)]
pub struct RunTracker {
    state: Arc<Mutex<Option<RunObservation>>>,
    last_request_started: Arc<Mutex<Option<Instant>>>,
}

pub(crate) struct RunObservation {
    pub(crate) started: Instant,
    pub(crate) model: String,
    pub(crate) tool_calls: usize,
    pub(crate) tool_errors: usize,
    pub(crate) tool_denials: usize,
    pub(crate) completion_calls: Vec<CompletionCall>,
    pub(crate) call_offsets_ms: Vec<u64>,
    pub(crate) last_request_id: Option<String>,
}

impl RunTracker {
    pub fn start(&self, model: &str) {
        *self.state() = Some(RunObservation {
            started: Instant::now(),
            model: model.to_string(),
            tool_calls: 0,
            tool_errors: 0,
            tool_denials: 0,
            completion_calls: Vec::new(),
            call_offsets_ms: Vec::new(),
            last_request_id: None,
        });
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

    /// Records a finished model request that was sent at `started`.
    pub fn completion(&self, call: CompletionCall, started: Instant) {
        *lock(&self.last_request_started) = Some(started);
        if let Some(state) = self.state().as_mut() {
            if let Some(ref req_id) = call.provider_request_id {
                state.last_request_id = Some(req_id.clone());
            }
            let offset = started.saturating_duration_since(state.started).as_millis();
            state.call_offsets_ms.push(u64::try_from(offset).unwrap_or(u64::MAX));
            state.completion_calls.push(call);
        }
    }

    /// When the most recent model request of any run was sent.
    pub fn last_request_started(&self) -> Option<Instant> {
        *lock(&self.last_request_started)
    }

    pub fn complete(&self, outcome: CompletionOutcome<'_>) -> RunMetrics {
        let observation = self.take();
        let usage = outcome.response.usage.is_reported().then_some(outcome.response.usage);
        let calls = call_metrics(&outcome.response.completion_calls, observation.as_ref());
        metrics_from_observation(ObservationMetricsInput {
            observation,
            session_id: outcome.session_id,
            status: outcome.status,
            usage,
            completion_calls: calls,
            requests: outcome.response.requests,
        })
    }

    pub fn terminate(&self, session_id: &str, status: TerminalStatus) -> RunMetrics {
        let observation = self.take();
        let usage = observation.as_ref().and_then(|state| {
            let usage = state
                .completion_calls
                .iter()
                .fold(StructuralUsage::default(), |total, call| total.merge(&call.usage));
            usage.is_reported().then_some(usage)
        });
        let calls = observation
            .as_ref()
            .map(|state| call_metrics(&state.completion_calls, Some(state)))
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
        lock(&self.state)
    }

    fn take(&self) -> Option<RunObservation> {
        self.state().take()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn call_metrics(calls: &[CompletionCall], observation: Option<&RunObservation>) -> Vec<ModelCallMetrics> {
    calls
        .iter()
        .enumerate()
        .map(|(index, call)| ModelCallMetrics {
            started_ms: observation.and_then(|state| state.call_offsets_ms.get(index).copied()),
            ..ModelCallMetrics::from(call)
        })
        .collect()
}
