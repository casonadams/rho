mod auto_compact;
mod completion;
mod prepare;
pub(crate) mod prune;
mod stream;
mod streaming_tool;
mod tool_hook;
mod truncation_hook;
pub mod types;

pub use prune::{DEFAULT_PRUNE_LINE_THRESHOLD, PrunePolicy, prune_historical_tool_outputs};
pub use types::{
    ActiveModelSwitch, CancellationSignal, QUEUED_MESSAGE_BOUNDARY, QueuedMessageBoundary, RunStatus,
    SharedModelSwitch, SteeringQueueProvider, TurnOutput, TurnRequest, UsageDetails,
};

use std::sync::Arc;

use crate::engine::AgentEngine;
use prepare::TurnLoopState;
use rho_harness_core::error::Result;
use rho_harness_core::presentation::presenter::Presenter;
use stream::StreamRunResult;

use super::sink::TerminalApprovalSink;

impl AgentEngine {
    async fn handle_budget_continue(
        &self,
        sink: &Arc<TerminalApprovalSink>,
        presenter: &dyn Presenter,
        loop_state: &mut TurnLoopState,
    ) -> Result<Option<TurnOutput>> {
        sink.resume_model_spinner();
        loop_state.current_prompt = "Please continue where you left off and finish the task.".to_string();
        loop_state.current_budget = 50;
        let additional_tokens =
            rho_harness_core::tokens::estimate_text_tokens(&loop_state.current_prompt, &self.config.model);
        if self
            .check_proactive_compaction(presenter, &mut loop_state.visible_history, additional_tokens)
            .await?
            .is_some()
        {
            return self.compacted_turn_output().await.map(Some);
        }
        Ok(None)
    }

    async fn handle_stream_run_result(
        &self,
        res: StreamRunResult,
        sink: &Arc<TerminalApprovalSink>,
        presenter: &dyn Presenter,
        loop_state: &mut TurnLoopState,
    ) -> Result<Option<TurnOutput>> {
        match res {
            StreamRunResult::RateLimitRetry | StreamRunResult::NetworkRetry => Ok(None),
            StreamRunResult::Compacted => self.compacted_turn_output().await.map(Some),
            StreamRunResult::BudgetContinue => self.handle_budget_continue(sink, presenter, loop_state).await,
            StreamRunResult::Complete(state) => {
                loop_state.rate_limit_retries = 0;
                loop_state.network_retries = 0;
                let out = self
                    .finalize_turn_execution(*state, sink, loop_state.checkpoint.as_deref())
                    .await?;
                Ok(Some(out))
            }
            StreamRunResult::ContentFiltered(state) => {
                loop_state.rate_limit_retries = 0;
                loop_state.network_retries = 0;
                let elapsed = Self::elapsed_generation_ms(&state);
                sink.finish_spinner();
                sink.flush_display();
                let prompt_response = rig::agent::PromptResponse::new("", rig::completion::Usage::new());
                let mut out = self
                    .finish_turn(crate::engine::runner::sink::TurnArtifacts {
                        response: prompt_response,
                        tool_calls_count: state.total_tool_calls,
                        completed_tools: sink.completed(),
                        generation_elapsed_ms: elapsed,
                    })
                    .await?;
                out.status = crate::engine::runner::turn::types::RunStatus::ContentFiltered;
                Ok(Some(out))
            }
        }
    }

    async fn execute_turn_step(
        &self,
        sink: &Arc<TerminalApprovalSink>,
        preamble: &str,
        request: &TurnRequest<'_>,
        presenter: &Arc<dyn Presenter>,
        loop_state: &mut TurnLoopState,
    ) -> Result<Option<TurnOutput>> {
        let (runner, active_model) = self
            .prepare_step_runner(sink, preamble, request, presenter, loop_state)
            .await?;
        let stream_res = self
            .run_turn_stream(runner, sink, presenter.as_ref(), &active_model, loop_state)
            .await?;
        self.handle_stream_run_result(stream_res, sink, presenter.as_ref(), loop_state)
            .await
    }

    pub async fn run_turn(&self, mut request: TurnRequest<'_>, presenter: Arc<dyn Presenter>) -> Result<TurnOutput> {
        self.apply_model_routing(&mut request, presenter.as_ref()).await;
        let mut prep = match self.prepare_turn(request.prompt, &presenter).await? {
            prepare::PreparedTurnOutcome::Compacted(out) => return Ok(*out),
            prepare::PreparedTurnOutcome::Ready(prep) => prep,
        };
        let _in_flight_guard = self.usage.in_flight_guard();
        loop {
            if let Some(out) = self
                .execute_turn_step(&prep.sink, &prep.preamble, &request, &presenter, &mut prep.loop_state)
                .await?
            {
                return Ok(out);
            }
        }
    }
}
