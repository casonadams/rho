use super::types::{
    CompletionCall, CompletionOutcome, ModelCallMetrics, ObservationMetricsInput, PruneDecision, RunMetrics,
    StructuralUsage, TerminalStatus, metrics_from_observation,
};
use crate::engine::locking::lock;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone, Default)]
pub struct RunTracker {
    state: Arc<Mutex<Option<RunObservation>>>,
    last_request: Arc<Mutex<Option<SentRequest>>>,
}

/// What selects a provider's prompt cache besides the request content.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PromptCacheKey {
    pub provider: String,
    pub model: String,
    pub settings: Option<String>,
}

/// The most recent model request of any run.
#[derive(Clone, Debug)]
pub struct SentRequest {
    pub started: Instant,
    pub cache_key: PromptCacheKey,
    reset_noted: bool,
}

pub(crate) struct RunObservation {
    pub(crate) started: Instant,
    pub(crate) model: String,
    pub(crate) cache_key: PromptCacheKey,
    pub(crate) tool_calls: usize,
    pub(crate) tool_errors: usize,
    pub(crate) tool_denials: usize,
    pub(crate) completion_calls: Vec<CompletionCall>,
    pub(crate) call_offsets_ms: Vec<u64>,
    pub(crate) last_request_id: Option<String>,
    pub(crate) prune: Option<PruneDecision>,
}

impl RunTracker {
    pub fn start(&self, model: &str, cache_key: PromptCacheKey) {
        *self.state() = Some(RunObservation {
            started: Instant::now(),
            model: model.to_string(),
            cache_key,
            tool_calls: 0,
            tool_errors: 0,
            tool_denials: 0,
            completion_calls: Vec::new(),
            call_offsets_ms: Vec::new(),
            last_request_id: None,
            prune: None,
        });
    }

    pub fn pruned(&self, decision: PruneDecision) {
        if let Some(state) = self.state().as_mut() {
            state.prune = Some(decision);
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

    /// Records a finished model request that was sent at `started`.
    pub fn completion(&self, call: CompletionCall, started: Instant) {
        let mut state = self.state();
        *lock(&self.last_request) = Some(SentRequest {
            started,
            cache_key: state.as_ref().map(|s| s.cache_key.clone()).unwrap_or_default(),
            reset_noted: false,
        });
        if let Some(state) = state.as_mut() {
            if let Some(ref req_id) = call.provider_request_id {
                state.last_request_id = Some(req_id.clone());
            }
            let offset = started.saturating_duration_since(state.started).as_millis();
            state.call_offsets_ms.push(u64::try_from(offset).unwrap_or(u64::MAX));
            state.completion_calls.push(call);
        }
    }

    pub fn last_request(&self) -> Option<SentRequest> {
        lock(&self.last_request).clone()
    }

    /// Compaction rewrites the history, so the last request's prompt cache no longer matches it.
    pub fn forget_last_request(&self) {
        *lock(&self.last_request) = None;
    }

    /// True only the first time since the last request, so a cache reset is reported once.
    pub fn note_cache_reset(&self) -> bool {
        lock(&self.last_request)
            .as_mut()
            .is_some_and(|sent| !std::mem::replace(&mut sent.reset_noted, true))
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
