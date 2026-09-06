mod auto_compact;
mod completion;
mod prepare;
mod stream;
mod streaming_tool;
mod tool_hook;
pub mod types;

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
        (sink, presenter): (&Arc<TerminalApprovalSink>, &dyn Presenter),
        loop_state: &mut TurnLoopState,
    ) -> Result<Option<TurnOutput>> {
        sink.resume_model_spinner();
        loop_state.current_prompt = "Please continue where you left off and finish the task.".to_string();
        loop_state.current_budget = 50;
        let additional_tokens =
            rho_harness_core::tokens::estimate_text_tokens(&loop_state.current_prompt, &self.config.model);
        if self
            .check_proactive_compaction(presenter, (&mut loop_state.visible_history, additional_tokens))
            .await?
            .is_some()
        {
            return self.compacted_turn_output().await.map(Some);
        }
        Ok(None)
    }

    async fn handle_stream_run_result(
        &self,
        (res, sink, presenter): (StreamRunResult, &Arc<TerminalApprovalSink>, &dyn Presenter),
        loop_state: &mut TurnLoopState,
    ) -> Result<Option<TurnOutput>> {
        match res {
            StreamRunResult::Compacted => self.compacted_turn_output().await.map(Some),
            StreamRunResult::BudgetContinue => self.handle_budget_continue((sink, presenter), loop_state).await,
            StreamRunResult::Complete(state) => {
                let out = self
                    .finalize_turn_execution(*state, (sink, loop_state.checkpoint.as_deref()))
                    .await?;
                Ok(Some(out))
            }
        }
    }

    async fn execute_turn_step(
        &self,
        (sink, preamble, request, presenter): (&Arc<TerminalApprovalSink>, &str, &TurnRequest<'_>, &Arc<dyn Presenter>),
        loop_state: &mut TurnLoopState,
    ) -> Result<Option<TurnOutput>> {
        let (runner, active_model) = self
            .prepare_step_runner((sink, preamble, request, presenter), loop_state)
            .await?;
        let stream_res = self
            .run_turn_stream(
                (runner, sink, presenter.as_ref()),
                (
                    &active_model,
                    &mut loop_state.visible_history,
                    &mut loop_state.checkpoint,
                    &mut loop_state.overflow_recovered,
                ),
            )
            .await?;
        self.handle_stream_run_result((stream_res, sink, presenter.as_ref()), loop_state)
            .await
    }

    pub async fn run_turn(&self, request: TurnRequest<'_>, presenter: Arc<dyn Presenter>) -> Result<TurnOutput> {
        let mut prep = match self.prepare_turn(request.prompt, &presenter).await? {
            prepare::PreparedTurnOutcome::Compacted(out) => return Ok(*out),
            prepare::PreparedTurnOutcome::Ready(prep) => prep,
        };
        let _in_flight_guard = self.usage.in_flight_guard();
        loop {
            if let Some(out) = self
                .execute_turn_step((&prep.sink, &prep.preamble, &request, &presenter), &mut prep.loop_state)
                .await?
            {
                return Ok(out);
            }
        }
    }
}
