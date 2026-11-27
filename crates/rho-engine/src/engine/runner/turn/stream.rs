use std::sync::Arc;
use std::time::Instant;

use futures::StreamExt;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::presentation::presenter::Presenter;

use super::calls::TurnCalls;
use super::prepare::TurnLoopState;
use super::runner::{AgentRunner, ModelRetry, MultiTurnStreamItem, StreamEvent};
use super::streaming_tool::StreamingToolTracker;
use crate::engine::AgentEngine;
use crate::engine::compactor::MadeRoom;
use crate::engine::metrics::types::{CompletionCall, PromptResponse, StructuralUsage};
use crate::engine::runner::sink::TerminalApprovalSink;
use crate::engine::tracking::UsageTracker;

pub(super) struct TurnStreamState {
    model_call_start: Option<Instant>,
    first_token_at: Option<Instant>,
    first_call_ttft_ms: Option<u64>,
    total_generation_elapsed_ms: u64,
    final_response: Option<PromptResponse>,
    streaming_tool: StreamingToolTracker,
    pub(super) total_tool_calls: usize,
    pub(super) cache_miss_tokens: Option<u64>,
    calls: TurnCalls,
}

impl TurnStreamState {
    pub(super) fn new() -> Self {
        Self {
            model_call_start: Some(Instant::now()),
            first_token_at: None,
            first_call_ttft_ms: None,
            total_generation_elapsed_ms: 0,
            final_response: None,
            streaming_tool: StreamingToolTracker::default(),
            total_tool_calls: 0,
            cache_miss_tokens: None,
            calls: TurnCalls::default(),
        }
    }

    /// Ends the run in progress to start another after a compaction; returns its request count.
    pub(super) fn next_run(&mut self) -> usize {
        self.restart_model_call();
        self.calls.next_run()
    }

    pub(super) fn elapsed_generation_ms(&self) -> u64 {
        (self.total_generation_elapsed_ms
            + self
                .model_call_start
                .map(|s| s.elapsed().as_millis() as u64)
                .unwrap_or(0))
        .max(1)
    }

    pub(super) fn ttft_ms(&self) -> Option<u64> {
        self.first_call_ttft_ms.or_else(|| self.call_ttft_ms())
    }

    /// Time to the first token of the current model call, which may have just finished.
    fn call_ttft_ms(&self) -> Option<u64> {
        let first = self.first_token_at?;
        Some(first.saturating_duration_since(self.model_call_start?).as_millis() as u64)
    }

    fn mark_first_token(&mut self, non_empty: bool) {
        if non_empty && self.first_token_at.is_none() {
            self.first_token_at = Some(Instant::now());
        }
    }

    fn restart_model_call(&mut self) {
        self.first_token_at = None;
        self.model_call_start = Some(Instant::now());
    }
}

pub(super) enum StreamRunResult {
    Complete(Box<TurnStreamState>),
    ContentFiltered(Box<TurnStreamState>),
}

impl StreamRunResult {
    pub(super) fn is_filtered(&self) -> bool {
        matches!(self, Self::ContentFiltered(_))
    }

    /// The turn's state and whole response; a filtered turn keeps its requests but no output.
    pub(super) fn into_response(self) -> (Box<TurnStreamState>, PromptResponse) {
        match self {
            Self::Complete(mut state) => {
                let response = state.final_response.take().unwrap_or_default();
                (state, response)
            }
            Self::ContentFiltered(state) => {
                let response = state
                    .calls
                    .whole_turn(PromptResponse::new("", StructuralUsage::default()));
                (state, response)
            }
        }
    }
}

/// How one run of the turn loop ended: the turn is over, or it made room in its context and
/// goes on with another run.
pub(super) enum RunEnd {
    Finished(StreamRunResult),
    MadeRoom(Box<TurnStreamState>),
}

/// The prompt that resumes a turn whose saved work is already in its history.
const RESUME_PROMPT: &str = "[The turn paused to make room in its context; continue the task from where it stopped.]";

fn format_model_retry(retry: &ModelRetry) -> String {
    format!(
        "[Model request failed ({}); retrying in {}s ({}/{})...]",
        retry.reason,
        retry.delay.as_secs(),
        retry.attempt,
        retry.max_attempts - 1
    )
}

fn record_streaming_text(text: &str, usage: &UsageTracker, state: &mut TurnStreamState) {
    state.mark_first_token(!text.is_empty());
    state.model_call_start.get_or_insert_with(Instant::now);
    let delta = rho_harness_core::tokens::estimate_char_tokens(text) as u64;
    usage.record_streaming_chunk(delta.max(1));
}

fn record_completion_call(
    call: &CompletionCall,
    usage: &UsageTracker,
    run_tracker: &crate::engine::metrics::RunTracker,
    state: &mut TurnStreamState,
) {
    if let Some(ttft) = state.call_ttft_ms() {
        state.first_call_ttft_ms.get_or_insert(ttft);
    }
    let started = state.model_call_start.take();
    state.first_token_at = None;
    let elapsed_ms = if let Some(s) = started {
        let ms = s.elapsed().as_millis().max(1) as u64;
        state.total_generation_elapsed_ms += ms;
        ms
    } else {
        0
    };
    usage.record_step(call.usage, elapsed_ms);
    if call.usage.is_reported()
        && let Some(miss) = usage.check_and_update_cache_miss(&call.usage)
    {
        state.cache_miss_tokens = Some(state.cache_miss_tokens.unwrap_or(0).saturating_add(miss));
    }
    run_tracker.completion(call.clone(), started.unwrap_or_else(Instant::now));
}

fn handle_completion_stream_item(
    item: MultiTurnStreamItem,
    usage: &UsageTracker,
    run_tracker: &crate::engine::metrics::RunTracker,
    sink: &Arc<TerminalApprovalSink>,
    state: &mut TurnStreamState,
) {
    state.streaming_tool.observe_boundary(&item, sink);
    match item {
        MultiTurnStreamItem::FinalResponse(resp) => {
            state.final_response = Some(state.calls.whole_turn(resp));
        }
        MultiTurnStreamItem::CompletionCall(call) => {
            let call = state.calls.record(call);
            record_completion_call(&call, usage, run_tracker, state);
        }
        MultiTurnStreamItem::ToolCall { .. } => {
            state.mark_first_token(true);
            state.total_tool_calls += 1;
        }
        MultiTurnStreamItem::ModelTurnRetried(retry) => {
            state.total_tool_calls = state.total_tool_calls.saturating_sub(retry.discarded_tool_calls);
            sink.finish_spinner();
            sink.flush_display();
            sink.presenter.print_notice(&format_model_retry(&retry));
            state.restart_model_call();
            sink.resume_model_spinner();
        }
        MultiTurnStreamItem::ToolExecutionCommitted => state.restart_model_call(),
        MultiTurnStreamItem::DisplayBarrier(drained) => drained.notify_one(),
        _ => {}
    }
}

impl AgentEngine {
    fn process_assistant_stream_item(
        &self,
        event: StreamEvent,
        sink: &Arc<TerminalApprovalSink>,
        state: &mut TurnStreamState,
    ) {
        match event {
            StreamEvent::Arguments { id, name, json } => {
                state.mark_first_token(!json.is_empty());
                sink.flush_reasoning();
                sink.resume_model_spinner();
                state.streaming_tool.handle_delta(&id, &name, &json, sink);
            }
            StreamEvent::Text(text) => {
                record_streaming_text(&text, &self.usage, state);
                sink.emit_text(&text);
            }
            StreamEvent::Reasoning(text) => {
                record_streaming_text(&text, &self.usage, state);
                sink.emit_reasoning(&text);
            }
        }
    }

    fn process_stream_item(
        &self,
        item: MultiTurnStreamItem,
        sink: &Arc<TerminalApprovalSink>,
        state: &mut TurnStreamState,
    ) {
        if let MultiTurnStreamItem::StreamAssistantItem(content) = item {
            self.process_assistant_stream_item(content, sink, state);
        } else {
            handle_completion_stream_item(item, &self.usage, &self.run_tracker, sink, state);
        }
    }

    /// Compacts the session mid-turn; true when it folded something. The turn then resumes its
    /// saved work, or sends its prompt again when none was saved.
    async fn compact_mid_turn(&self, presenter: &dyn Presenter, loop_state: &mut TurnLoopState) -> Result<bool> {
        let _spinner = presenter.start_spinner("Compacting...");
        let started_with = loop_state.checkpoint.as_ref().map_or(0, Vec::len);
        let saved_now = self.session_manager.load_checkpoint().await?.map_or(0, |c| c.len());
        match self.compact_session(None).await {
            Ok(stats) if !stats.summary.is_empty() => {
                loop_state.checkpoint = None;
                loop_state.visible_history = self.session_manager.active_messages().await?;
                if saved_now > started_with {
                    loop_state.current_prompt = RESUME_PROMPT.to_string();
                }
                presenter.print_notice(&stats.auto_notice());
                Ok(true)
            }
            Ok(_) => Ok(false),
            Err(e) => {
                presenter.print_notice(&format!("[Auto-compaction failed: {e}]"));
                Ok(false)
            }
        }
    }

    /// Readies the turn to go on after `error` stopped it for a full context; false when it cannot.
    async fn make_room(
        &self,
        error: &AppError,
        presenter: &dyn Presenter,
        loop_state: &mut TurnLoopState,
    ) -> Result<bool> {
        let mut compaction = loop_state.compaction;
        let made = compaction
            .make_room(error, async || {
                Ok::<_, AppError>(self.compact_mid_turn(presenter, loop_state).await?.then_some(()))
            })
            .await?;
        loop_state.compaction = compaction;
        match made {
            Some(MadeRoom::Compacted(())) => Ok(true),
            Some(MadeRoom::RunOn) => {
                loop_state.checkpoint = self.session_manager.load_checkpoint().await?;
                loop_state.current_prompt.clear();
                Ok(true)
            }
            None => Ok(false),
        }
    }

    async fn handle_fatal_stream_error(&self, error: AppError, sink: &Arc<TerminalApprovalSink>) -> Result<RunEnd> {
        if matches!(error, AppError::InvalidToolCall(_)) {
            self.run_tracker.invalid_tool();
        }
        self.record_failed_turn(&error, sink).await?;
        Err(error)
    }

    async fn handle_stream_error(
        &self,
        error: AppError,
        presenter: &dyn Presenter,
        sink: &Arc<TerminalApprovalSink>,
        loop_state: &mut TurnLoopState,
        state: TurnStreamState,
    ) -> Result<RunEnd> {
        sink.finish_spinner();
        sink.flush_display();
        if self.make_room(&error, presenter, loop_state).await? {
            return Ok(RunEnd::MadeRoom(Box::new(state)));
        }
        self.handle_fatal_stream_error(error, sink).await
    }

    pub(super) async fn run_turn_stream(
        &self,
        runner: AgentRunner,
        sink: &Arc<TerminalApprovalSink>,
        presenter: &dyn Presenter,
        loop_state: &mut TurnLoopState,
        mut state: TurnStreamState,
    ) -> Result<RunEnd> {
        let mut stream = runner.stream();
        while let Some(item) = stream.next().await {
            match item {
                Ok(item) => self.process_stream_item(item, sink, &mut state),
                Err(AppError::ContentFiltered) => {
                    return Ok(RunEnd::Finished(StreamRunResult::ContentFiltered(Box::new(state))));
                }
                Err(err) => return self.handle_stream_error(err, presenter, sink, loop_state, state).await,
            }
            tokio::task::yield_now().await;
        }
        Ok(RunEnd::Finished(StreamRunResult::Complete(Box::new(state))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::eval::mock::{MockEngineConfig, mock_engine};
    use crate::engine::eval::presenter::NoopPresenter;
    use crate::provider::adapter::MockAdapter;

    #[tokio::test]
    async fn fatal_stream_errors_are_recorded_with_their_cause() {
        let temp_dir = crate::test_support::TestDir::new();
        let engine = mock_engine(
            Arc::new(MockAdapter::default()),
            MockEngineConfig {
                base_dir: &temp_dir,
                app_config: rho_harness_core::config::Config::default(),
                session_manager: None,
                built_in_tools: None,
            },
        );
        let presenter: Arc<dyn Presenter> = Arc::new(NoopPresenter);
        let sink = engine.create_approval_sink(&presenter);
        let mut loop_state = TurnLoopState {
            visible_history: Vec::new(),
            checkpoint: None,
            current_prompt: String::new(),
            current_budget: 10,
            thinking_level: None,
            compaction: crate::engine::compactor::TurnCompaction::default(),
            turn_started_at: Instant::now(),
        };

        let error = AppError::Network("provider stream went idle for 300s without sending data".into());
        let result = engine
            .handle_stream_error(
                error,
                presenter.as_ref(),
                &sink,
                &mut loop_state,
                TurnStreamState::new(),
            )
            .await;

        assert!(matches!(result, Err(AppError::Network(_))));
        let events = engine.session_manager.load_events().await.unwrap();
        let failed = events
            .iter()
            .find(|e| e.kind == rho_harness_core::session::SessionEventKind::RunFailed)
            .expect("RunFailed event");
        assert!(failed.payload["error"].as_str().unwrap().contains("went idle for 300s"));
        let summary = events
            .iter()
            .find(|e| e.kind == rho_harness_core::session::SessionEventKind::RunSummary)
            .expect("run summary");
        assert_eq!(summary.payload["terminal_status"], "failed");
    }

    #[test]
    fn a_new_run_forgets_the_first_token_of_the_run_it_replaced() {
        let mut state = TurnStreamState::new();
        state.mark_first_token(true);
        state.next_run();
        assert_eq!(state.ttft_ms(), None);
    }

    #[test]
    fn a_filtered_turn_keeps_its_requests_but_drops_its_output() {
        let mut state = TurnStreamState::new();
        state.calls.record(CompletionCall::new(0, StructuralUsage::default()));
        state.final_response = Some(PromptResponse::new("hidden", StructuralUsage::default()));
        state.next_run();

        let (_, response) = StreamRunResult::ContentFiltered(Box::new(state)).into_response();

        assert_eq!(response.output, "");
        assert_eq!(response.requests, 1);
    }

    #[test]
    fn retry_notice_names_the_cause_delay_and_attempt() {
        let retry = ModelRetry {
            attempt: 1,
            max_attempts: 3,
            delay: std::time::Duration::from_secs(2),
            reason: "Network error: connection reset".into(),
            discarded_tool_calls: 0,
        };
        assert_eq!(
            format_model_retry(&retry),
            "[Model request failed (Network error: connection reset); retrying in 2s (1/2)...]"
        );
    }
}
