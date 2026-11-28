mod calls;
mod checkpoint;
mod completion;
mod prepare;
pub(crate) mod prune;
pub mod runner;
mod store;
mod stream;
pub(crate) mod streaming_tool;
mod thinking;
pub mod types;
pub(crate) mod wakeups;

pub use prune::{DEFAULT_PRUNE_LINE_THRESHOLD, PruneMemory, PrunePolicy, prune_historical_tool_outputs_with_policy};
pub use store::{MemoryStore, TurnStore};
pub use types::{
    CancellationSignal, QUEUED_MESSAGE_BOUNDARY, QueuedMessageBoundary, RunStatus, SteeringQueueProvider, TurnOutput,
    TurnRequest, UsageDetails,
};

use std::sync::Arc;
use std::time::Instant;

use crate::engine::AgentEngine;
use prepare::TurnLoopState;
use rho_harness_core::error::Result;
use rho_harness_core::presentation::presenter::Presenter;
use stream::{RunEnd, StreamRunResult, TurnStreamState};

use super::sink::{TerminalApprovalSink, TurnArtifacts};

impl AgentEngine {
    async fn handle_stream_run_result(
        &self,
        res: StreamRunResult,
        sink: &Arc<TerminalApprovalSink>,
        lifecycle: &crate::hook::LifecycleHook,
        turn_started_at: Instant,
    ) -> Result<TurnOutput> {
        sink.record_tool_events().await?;
        let content_filtered = res.is_filtered();
        let (state, response) = res.into_response();
        let generation_elapsed_ms = state.elapsed_generation_ms();
        let ttft_ms = state.ttft_ms();
        sink.finish_spinner();
        sink.flush_display();
        let artifacts = TurnArtifacts {
            response,
            tool_calls_count: state.total_tool_calls,
            completed_tools: sink.completed(),
            generation_elapsed_ms,
            turn_started_at,
            ttft_ms,
            cache_miss_tokens: state.cache_miss_tokens,
            content_filtered,
        };
        self.finish_turn(artifacts, lifecycle).await
    }

    /// Runs the turn loop until the turn is over, starting it again with the turns it has left
    /// each time it stops to make room in its context.
    async fn drive_turn(
        &self,
        sink: &Arc<TerminalApprovalSink>,
        preamble: &str,
        request: &TurnRequest<'_>,
        lifecycle: &Arc<crate::hook::LifecycleHook>,
        loop_state: &mut TurnLoopState,
    ) -> Result<TurnOutput> {
        let presenter = lifecycle.presenter();
        lifecycle.notify_turn_start(&loop_state.current_prompt).await;
        let mut state = TurnStreamState::new();
        loop {
            let runner = self
                .prepare_step_runner(sink, preamble, request, lifecycle, loop_state)
                .await?;
            match self
                .run_turn_stream(runner, sink, presenter.as_ref(), loop_state, state)
                .await?
            {
                RunEnd::MadeRoom(paused) => {
                    state = *paused;
                    loop_state.current_budget = loop_state.current_budget.saturating_sub(state.end_run());
                }
                RunEnd::Finished(result) => {
                    return self
                        .handle_stream_run_result(result, sink, lifecycle, loop_state.turn_started_at)
                        .await;
                }
            }
        }
    }

    pub async fn run_turn(&self, request: TurnRequest<'_>, presenter: Arc<dyn Presenter>) -> Result<TurnOutput> {
        let turn_started_at = Instant::now();
        let oauth_secrets = self.router.auth().lock().await.oauth_secret_values();
        self.session_manager.add_secrets_async(oauth_secrets).await?;
        let mut prep = self.prepare_turn(request.prompt, &presenter, turn_started_at).await?;
        let lifecycle = self.lifecycle_hook(presenter.clone());
        let _in_flight_guard = self.usage.in_flight_guard();
        self.drive_turn(&prep.sink, &prep.preamble, &request, &lifecycle, &mut prep.loop_state)
            .await
    }
}
