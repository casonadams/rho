use std::sync::Arc;
use std::time::Instant;

use futures::StreamExt;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::presentation::presenter::Presenter;

use super::prepare::TurnLoopState;
use super::runner::{AgentRunner, ModelRetry, MultiTurnStreamItem, StreamEvent};
use super::streaming_tool::StreamingToolTracker;
use crate::engine::AgentEngine;
use crate::engine::compactor::MAX_COMPACTIONS_PER_TURN;
use crate::engine::metrics::StructuralUsage;
use crate::engine::metrics::types::{CompletionCall, PromptResponse};
use crate::engine::runner::history::DisplayEvent;
use crate::engine::runner::sink::TerminalApprovalSink;
use crate::engine::tracking::UsageTracker;

pub(super) struct TurnStreamState {
    pub(super) model_call_start: Option<Instant>,
    pub(super) first_token_at: Option<Instant>,
    pub(super) ttft_ms: Option<u64>,
    pub(super) total_generation_elapsed_ms: u64,
    pub(super) final_response: Option<PromptResponse>,
    pub(super) streaming_tool: StreamingToolTracker,
    pub(super) total_tool_calls: usize,
    pub(super) cache_miss_tokens: Option<u64>,
    /// Requests of the turn's runs that ended to compact; the final response adds them.
    earlier_calls: Vec<CompletionCall>,
    /// Requests of the run in progress.
    run_calls: Vec<CompletionCall>,
}

impl TurnStreamState {
    pub(super) fn new() -> Self {
        Self {
            model_call_start: Some(Instant::now()),
            first_token_at: None,
            ttft_ms: None,
            total_generation_elapsed_ms: 0,
            final_response: None,
            streaming_tool: StreamingToolTracker::default(),
            total_tool_calls: 0,
            cache_miss_tokens: None,
            earlier_calls: Vec::new(),
            run_calls: Vec::new(),
        }
    }

    /// Ends the run in progress to start another after a compaction; returns its request count.
    pub(super) fn next_run(&mut self) -> usize {
        let requests = self.run_calls.len();
        self.earlier_calls.append(&mut self.run_calls);
        self.model_call_start = Some(Instant::now());
        requests
    }

    /// `call` of the run in progress, numbered among all the turn's requests.
    fn in_turn(&self, mut call: CompletionCall) -> CompletionCall {
        call.call_index += self.earlier_calls.len();
        call
    }

    /// The final response of the last run, with the requests of the runs before it.
    pub(super) fn whole_turn(&self, last: PromptResponse) -> PromptResponse {
        if self.earlier_calls.is_empty() {
            return last;
        }
        let earlier = self
            .earlier_calls
            .iter()
            .fold(StructuralUsage::default(), |sum, call| sum.merge(&call.usage));
        let mut completion_calls = self.earlier_calls.clone();
        completion_calls.extend(last.completion_calls.into_iter().map(|call| self.in_turn(call)));
        PromptResponse {
            output: last.output,
            usage: earlier.merge(&last.usage),
            requests: self.earlier_calls.len() + last.requests,
            completion_calls,
        }
    }
}

fn mark_first_token(first_token_at: &mut Option<Instant>, non_empty: bool) {
    if non_empty && first_token_at.is_none() {
        *first_token_at = Some(Instant::now());
    }
}

pub(super) enum StreamRunResult {
    Complete(Box<TurnStreamState>),
    ContentFiltered(Box<TurnStreamState>),
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

fn record_streaming_text(text: &str, usage: &UsageTracker, start: &mut Option<Instant>) {
    if start.is_none() {
        *start = Some(Instant::now());
    }
    let delta = rho_harness_core::tokens::estimate_char_tokens(text) as u64;
    usage.record_streaming_chunk(delta.max(1));
}

fn handle_display_events(
    events: Vec<DisplayEvent>,
    sink: &Arc<TerminalApprovalSink>,
    usage: &UsageTracker,
    start: &mut Option<Instant>,
    tool_calls: &mut usize,
) {
    for event in events {
        match event {
            DisplayEvent::Text(text) => {
                record_streaming_text(&text, usage, start);
                sink.emit_text(&text);
            }
            DisplayEvent::Reasoning(text) => {
                record_streaming_text(&text, usage, start);
                sink.emit_reasoning(&text);
            }
            DisplayEvent::ToolCall { .. } => {
                sink.flush_reasoning();
                sink.resume_model_spinner();
                *tool_calls += 1;
            }
        }
    }
}

fn record_completion_call(
    call: &CompletionCall,
    usage: &UsageTracker,
    run_tracker: &crate::engine::metrics::RunTracker,
    state: &mut TurnStreamState,
) {
    let started = state.model_call_start.take();
    let first_tok = state.first_token_at.take();
    if let (Some(s), Some(f)) = (started, first_tok) {
        let ttft = f.saturating_duration_since(s).as_millis() as u64;
        state.ttft_ms.get_or_insert(ttft);
    }
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
            state.final_response = Some(state.whole_turn(resp));
        }
        MultiTurnStreamItem::CompletionCall(call) => {
            let call = state.in_turn(call);
            record_completion_call(&call, usage, run_tracker, state);
            state.run_calls.push(call);
        }
        MultiTurnStreamItem::ToolCall { .. } => {
            mark_first_token(&mut state.first_token_at, true);
            state.total_tool_calls += 1;
        }
        MultiTurnStreamItem::ModelTurnRetried(retry) => {
            state.total_tool_calls = state.total_tool_calls.saturating_sub(retry.discarded_tool_calls);
            sink.finish_spinner();
            sink.flush_display();
            sink.presenter.print_notice(&format_model_retry(&retry));
            state.first_token_at = None;
            state.model_call_start = Some(Instant::now());
            sink.resume_model_spinner();
        }
        MultiTurnStreamItem::ToolExecutionCommitted => {
            state.first_token_at = None;
            state.model_call_start = Some(Instant::now());
        }
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
                mark_first_token(&mut state.first_token_at, !json.is_empty());
                sink.flush_reasoning();
                sink.resume_model_spinner();
                state.streaming_tool.handle_delta(&id, &name, &json, sink);
            }
            StreamEvent::Text(t) => {
                mark_first_token(&mut state.first_token_at, !t.is_empty());
                let events = vec![DisplayEvent::Text(t)];
                handle_display_events(
                    events,
                    sink,
                    &self.usage,
                    &mut state.model_call_start,
                    &mut state.total_tool_calls,
                );
            }
            StreamEvent::Reasoning(r) => {
                mark_first_token(&mut state.first_token_at, !r.is_empty());
                let events = vec![DisplayEvent::Reasoning(r)];
                handle_display_events(
                    events,
                    sink,
                    &self.usage,
                    &mut state.model_call_start,
                    &mut state.total_tool_calls,
                );
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

    /// Compacts the session mid-turn; true when it folded something, so the turn goes on from
    /// the compacted history (resuming its saved work, or sending its prompt again when none was
    /// saved).
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

    /// Readies the turn to go on after `error` stopped it for a full context: compacted, or, when
    /// an early stop finds nothing to compact, resumed without early stops. False when it cannot.
    async fn make_room(
        &self,
        error: &AppError,
        presenter: &dyn Presenter,
        loop_state: &mut TurnLoopState,
    ) -> Result<bool> {
        if !error.needs_compaction() {
            return Ok(false);
        }
        if loop_state.compactions < MAX_COMPACTIONS_PER_TURN && self.compact_mid_turn(presenter, loop_state).await? {
            loop_state.compactions += 1;
            return Ok(true);
        }
        if !matches!(error, AppError::CompactionDue { .. }) {
            return Ok(false);
        }
        loop_state.compact_early = false;
        loop_state.checkpoint = self.session_manager.load_checkpoint().await?;
        loop_state.current_prompt = RESUME_PROMPT.to_string();
        Ok(true)
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

    pub(super) fn elapsed_generation_ms(state: &TurnStreamState) -> u64 {
        (state.total_generation_elapsed_ms
            + state
                .model_call_start
                .map(|s| s.elapsed().as_millis() as u64)
                .unwrap_or(0))
        .max(1)
    }

    pub(super) fn ttft_ms(state: &TurnStreamState) -> Option<u64> {
        state.ttft_ms.or_else(|| {
            let start = state.model_call_start?;
            let first = state.first_token_at?;
            Some(first.saturating_duration_since(start).as_millis() as u64)
        })
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
        let temp_dir = tempfile::tempdir().unwrap();
        let engine = mock_engine(
            Arc::new(MockAdapter::default()),
            MockEngineConfig {
                base_dir: temp_dir.path(),
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
            compactions: 0,
            compact_early: true,
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
