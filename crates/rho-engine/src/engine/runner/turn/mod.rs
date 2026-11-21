mod completion;
mod prepare;
pub(crate) mod prune;
pub mod runner;
pub mod store;
mod stream;
mod streaming_tool;
pub mod types;

pub use prune::{DEFAULT_PRUNE_LINE_THRESHOLD, PrunePolicy, prune_historical_tool_outputs_with_policy};
pub use store::{MemoryStore, TurnStore};
pub use types::{
    CancellationSignal, QUEUED_MESSAGE_BOUNDARY, QueuedMessageBoundary, RunStatus, SteeringQueueProvider, TurnOutput,
    TurnRequest, UsageDetails,
};

use std::sync::Arc;

use crate::engine::AgentEngine;
use prepare::TurnLoopState;
use rho_harness_core::error::Result;
use rho_harness_core::presentation::presenter::Presenter;
use stream::StreamRunResult;

use super::sink::TerminalApprovalSink;

impl AgentEngine {
    async fn handle_stream_run_result(
        &self,
        res: StreamRunResult,
        sink: &Arc<TerminalApprovalSink>,
        lifecycle: &crate::hook::LifecycleHook,
    ) -> Result<TurnOutput> {
        sink.record_tool_events().await?;
        match res {
            StreamRunResult::Compacted(status) => self.compacted_turn_output(status).await,
            StreamRunResult::Complete(state) => {
                let elapsed = Self::elapsed_generation_ms(&state);
                sink.finish_spinner();
                sink.flush_display();
                let prompt_response = state.final_response.unwrap_or_default();
                self.finish_turn(
                    crate::engine::runner::sink::TurnArtifacts {
                        response: prompt_response,
                        tool_calls_count: state.total_tool_calls,
                        completed_tools: sink.completed(),
                        generation_elapsed_ms: elapsed,
                    },
                    lifecycle,
                )
                .await
            }
            StreamRunResult::ContentFiltered(state) => {
                let elapsed = Self::elapsed_generation_ms(&state);
                sink.finish_spinner();
                sink.flush_display();
                let prompt_response = crate::engine::metrics::types::PromptResponse::new(
                    "",
                    crate::engine::metrics::types::StructuralUsage::default(),
                );
                let mut out = self
                    .finish_turn(
                        crate::engine::runner::sink::TurnArtifacts {
                            response: prompt_response,
                            tool_calls_count: state.total_tool_calls,
                            completed_tools: sink.completed(),
                            generation_elapsed_ms: elapsed,
                        },
                        lifecycle,
                    )
                    .await?;
                out.status = crate::engine::runner::turn::types::RunStatus::ContentFiltered;
                Ok(out)
            }
        }
    }

    async fn execute_turn_step(
        &self,
        sink: &Arc<TerminalApprovalSink>,
        preamble: &str,
        request: &TurnRequest<'_>,
        lifecycle: &Arc<crate::hook::LifecycleHook>,
        loop_state: &mut TurnLoopState,
    ) -> Result<TurnOutput> {
        let presenter = lifecycle.presenter();
        let runner = self
            .prepare_step_runner(sink, preamble, request, lifecycle, loop_state)
            .await?;
        let stream_res = self
            .run_turn_stream(runner, sink, presenter.as_ref(), loop_state)
            .await?;
        self.handle_stream_run_result(stream_res, sink, lifecycle).await
    }

    pub async fn run_turn(&self, request: TurnRequest<'_>, presenter: Arc<dyn Presenter>) -> Result<TurnOutput> {
        let oauth_secrets = self.router.auth().lock().await.oauth_secret_values();
        self.session_manager.add_secrets_async(oauth_secrets).await?;
        let mut prep = match self.prepare_turn(request.prompt, &presenter).await? {
            prepare::PreparedTurnOutcome::Compacted(out) => return Ok(*out),
            prepare::PreparedTurnOutcome::Ready(prep) => prep,
        };
        let lifecycle = self.lifecycle_hook(presenter.clone());
        let _in_flight_guard = self.usage.in_flight_guard();
        self.execute_turn_step(&prep.sink, &prep.preamble, &request, &lifecycle, &mut prep.loop_state)
            .await
    }
}
