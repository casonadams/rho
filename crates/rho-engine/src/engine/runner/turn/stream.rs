use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use futures::StreamExt;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::presentation::presenter::Presenter;
use rig::agent::{AgentRunner, CompletionCall, MultiTurnStreamItem, PromptResponse, StreamingError};
use rig::memory::ConversationMemory;
use rig::message::Message;
use rig::streaming::StreamedAssistantContent;

use super::streaming_tool::StreamingToolTracker;
use super::types::TurnOutput;
use crate::engine::AgentEngine;
use crate::engine::runner::history::{
    DisplayEvent, budget_history, checkpoint_messages, display_events, map_streaming_error,
};
use crate::engine::runner::sink::{TerminalApprovalSink, TurnArtifacts};
use crate::engine::tracking::UsageTracker;

pub(super) struct TurnStreamState {
    pub(super) model_call_start: Option<Instant>,
    pub(super) total_generation_elapsed_ms: u64,
    pub(super) final_response: Option<PromptResponse>,
    pub(super) reasoning_parts: HashSet<String>,
    pub(super) streaming_tool: StreamingToolTracker,
    pub(super) total_tool_calls: usize,
}

impl TurnStreamState {
    fn new() -> Self {
        Self {
            model_call_start: Some(Instant::now()),
            total_generation_elapsed_ms: 0,
            final_response: None,
            reasoning_parts: HashSet::new(),
            streaming_tool: StreamingToolTracker::default(),
            total_tool_calls: 0,
        }
    }
}

pub(super) enum StreamRunResult {
    RetryOverflow,
    BudgetContinue,
    Complete(Box<TurnStreamState>),
}

enum StreamErrorAction {
    RetryOverflow,
    BudgetContinue,
}

fn record_streaming_text(text: &str, model: &str, (usage, start): (&UsageTracker, &mut Option<Instant>)) {
    if start.is_none() {
        *start = Some(Instant::now());
    }
    let delta = rho_harness_core::tokens::estimate_text_tokens(text, model) as u64;
    usage.record_streaming_chunk(delta);
}

fn handle_display_events(
    events: Vec<DisplayEvent>,
    sink: &Arc<TerminalApprovalSink>,
    (usage, model, start, tool_calls): (&UsageTracker, &str, &mut Option<Instant>, &mut usize),
) {
    for event in events {
        match event {
            DisplayEvent::Text(text) => {
                record_streaming_text(&text, model, (usage, start));
                sink.emit_text(&text);
            }
            DisplayEvent::Reasoning(text) => {
                record_streaming_text(&text, model, (usage, start));
                sink.emit_reasoning(&text);
            }
            DisplayEvent::ToolCall { .. } => {
                sink.resume_model_spinner();
                *tool_calls += 1;
            }
        }
    }
}

fn record_completion_call(
    call: &CompletionCall,
    (usage, run_tracker): (&UsageTracker, &crate::engine::metrics::RunTracker),
    (start, total_elapsed): (&mut Option<Instant>, &mut u64),
) {
    let elapsed_ms = if let Some(s) = start.take() {
        let ms = s.elapsed().as_millis().max(1) as u64;
        *total_elapsed += ms;
        ms
    } else {
        0
    };
    usage.record_step(call.usage.into(), elapsed_ms);
    run_tracker.completion(call.clone());
}

fn handle_completion_stream_item(
    item: MultiTurnStreamItem,
    (usage, run_tracker, sink): (
        &UsageTracker,
        &crate::engine::metrics::RunTracker,
        &Arc<TerminalApprovalSink>,
    ),
    state: &mut TurnStreamState,
) {
    match item {
        MultiTurnStreamItem::FinalResponse(resp) => {
            state.streaming_tool.reset();
            state.final_response = Some(resp);
        }
        MultiTurnStreamItem::CompletionCall(call) => {
            state.streaming_tool.reset();
            record_completion_call(
                &call,
                (usage, run_tracker),
                (&mut state.model_call_start, &mut state.total_generation_elapsed_ms),
            );
        }
        MultiTurnStreamItem::ModelTurnRetried { .. } => {
            state.model_call_start = Some(Instant::now());
            sink.resume_model_spinner();
        }
        MultiTurnStreamItem::ToolExecutionCommitted { .. } => {
            state.streaming_tool.reset();
            state.model_call_start = Some(Instant::now());
        }
        _ => {}
    }
}

impl AgentEngine {
    fn process_assistant_stream_item(
        &self,
        content: StreamedAssistantContent,
        (sink, state, active_model): (&Arc<TerminalApprovalSink>, &mut TurnStreamState, &str),
    ) {
        if let StreamedAssistantContent::ToolCallDelta { content, .. } = content {
            sink.resume_model_spinner();
            state.streaming_tool.handle_delta(content, sink);
        } else {
            let events = display_events(content, &mut state.reasoning_parts);
            handle_display_events(
                events,
                sink,
                (
                    &self.usage,
                    active_model,
                    &mut state.model_call_start,
                    &mut state.total_tool_calls,
                ),
            );
        }
    }

    fn process_stream_item(
        &self,
        item: MultiTurnStreamItem,
        (sink, state, active_model): (&Arc<TerminalApprovalSink>, &mut TurnStreamState, &str),
    ) {
        if let MultiTurnStreamItem::StreamAssistantItem(content) = item {
            self.process_assistant_stream_item(content, (sink, state, active_model));
        } else {
            handle_completion_stream_item(item, (&self.usage, &self.run_tracker, sink), state);
        }
    }

    async fn try_recover_overflow(
        &self,
        (presenter, sink): (&dyn Presenter, &Arc<TerminalApprovalSink>),
        (visible_history, checkpoint): (&mut Vec<Message>, &mut Option<Vec<Message>>),
    ) -> Result<bool> {
        presenter.print_notice("[Context overflow detected: auto-compacting and retrying turn...]");
        let spinner = presenter.start_spinner("Compacting...");
        let stats = match self.compact_session(None).await {
            Ok(s) => s,
            Err(e) => {
                spinner.finish_and_clear();
                eprintln!("Warning: Auto-compaction after context overflow failed: {e}");
                return Ok(false);
            }
        };
        spinner.finish_and_clear();
        presenter.print_notice(&format!(
            "[Compacted context: {} -> {} tokens (saved {})]",
            stats.tokens_before, stats.tokens_after, stats.saved_tokens
        ));
        *visible_history = ConversationMemory::load(&self.session_manager, &self.session_manager.session_id)
            .await
            .map_err(|e| AppError::Session(format!("Model-visible session history could not be loaded: {e}")))?;
        *checkpoint = self.session_manager.load_checkpoint().await?;
        sink.resume_model_spinner();
        Ok(true)
    }

    async fn handle_budget_continuation(
        &self,
        presenter: &dyn Presenter,
        (turns, hist, vis_hist, chk): (usize, &[Message], &[Message], &mut Option<Vec<Message>>),
    ) -> Result<bool> {
        let pending = checkpoint_messages(vis_hist, hist)?;
        self.session_manager.save_checkpoint(pending.clone()).await?;
        *chk = Some(pending);
        Ok(presenter.prompt_continue_budget(turns).await)
    }

    async fn try_context_overflow(
        &self,
        (error, presenter, sink): (&StreamingError, &dyn Presenter, &Arc<TerminalApprovalSink>),
        (visible_history, checkpoint, overflow_recovered): (&mut Vec<Message>, &mut Option<Vec<Message>>, &mut bool),
    ) -> Result<bool> {
        if !*overflow_recovered && crate::engine::compactor::is_context_overflow_error(error) {
            *overflow_recovered = true;
            return self
                .try_recover_overflow((presenter, sink), (visible_history, checkpoint))
                .await;
        }
        Ok(false)
    }

    async fn try_budget_continuation(
        &self,
        (error, presenter): (&StreamingError, &dyn Presenter),
        (visible_history, checkpoint): (&[Message], &mut Option<Vec<Message>>),
    ) -> Result<bool> {
        if let Some((turns, hist)) = budget_history(error) {
            return self
                .handle_budget_continuation(presenter, (turns, &hist, visible_history, checkpoint))
                .await;
        }
        Ok(false)
    }

    async fn handle_fatal_stream_error(&self, error: StreamingError) -> Result<StreamErrorAction> {
        let err = map_streaming_error(error);
        if matches!(err, AppError::InvalidToolCall(_)) {
            self.run_tracker.invalid_tool();
        }
        self.record_failed_metrics(&err).await?;
        Err(err)
    }

    async fn handle_stream_error(
        &self,
        (error, presenter, sink): (StreamingError, &dyn Presenter, &Arc<TerminalApprovalSink>),
        (visible_history, checkpoint, overflow_recovered): (&mut Vec<Message>, &mut Option<Vec<Message>>, &mut bool),
    ) -> Result<StreamErrorAction> {
        sink.finish_spinner();
        sink.flush_display();
        if self
            .try_context_overflow(
                (&error, presenter, sink),
                (visible_history, checkpoint, overflow_recovered),
            )
            .await?
        {
            return Ok(StreamErrorAction::RetryOverflow);
        }
        if let Some(mem_err) = self.session_manager.take_memory_error() {
            let err = AppError::Session(mem_err);
            self.record_failed_metrics(&err).await?;
            return Err(err);
        }
        if self
            .try_budget_continuation((&error, presenter), (visible_history, checkpoint))
            .await?
        {
            return Ok(StreamErrorAction::BudgetContinue);
        }
        self.handle_fatal_stream_error(error).await
    }

    pub(super) async fn run_turn_stream(
        &self,
        (runner, sink, presenter): (AgentRunner, &Arc<TerminalApprovalSink>, &dyn Presenter),
        (active_model, visible_history, checkpoint, overflow_recovered): (
            &str,
            &mut Vec<Message>,
            &mut Option<Vec<Message>>,
            &mut bool,
        ),
    ) -> Result<StreamRunResult> {
        let mut state = TurnStreamState::new();
        let mut stream = runner.stream().await;
        while let Some(item) = stream.next().await {
            match item {
                Ok(item) => self.process_stream_item(item, (sink, &mut state, active_model)),
                Err(err) => {
                    let action = self
                        .handle_stream_error(
                            (err, presenter, sink),
                            (visible_history, checkpoint, overflow_recovered),
                        )
                        .await?;
                    match action {
                        StreamErrorAction::RetryOverflow => return Ok(StreamRunResult::RetryOverflow),
                        StreamErrorAction::BudgetContinue => return Ok(StreamRunResult::BudgetContinue),
                    }
                }
            }
        }
        Ok(StreamRunResult::Complete(Box::new(state)))
    }

    async fn promote_continuation_checkpoint(
        &self,
        messages: Option<Vec<Message>>,
        checkpoint: Option<&[Message]>,
    ) -> Result<()> {
        if checkpoint.is_some() {
            let messages = messages.ok_or_else(|| {
                AppError::Session("Completed continuation did not return canonical messages".to_string())
            })?;
            self.session_manager.promote_checkpoint(messages).await?;
        }
        Ok(())
    }

    fn elapsed_generation_ms(state: &TurnStreamState) -> u64 {
        (state.total_generation_elapsed_ms
            + state
                .model_call_start
                .map(|s| s.elapsed().as_millis() as u64)
                .unwrap_or(0))
        .max(1)
    }

    fn check_memory_error(&self) -> Result<()> {
        if let Some(err) = self.session_manager.take_memory_error() {
            return Err(AppError::Session(err));
        }
        Ok(())
    }

    pub(super) async fn finalize_turn_execution(
        &self,
        state: TurnStreamState,
        (sink, checkpoint): (&Arc<TerminalApprovalSink>, Option<&[Message]>),
    ) -> Result<TurnOutput> {
        let elapsed = Self::elapsed_generation_ms(&state);
        sink.finish_spinner();
        sink.flush_display();
        let Some(response) = state.final_response else {
            let err = AppError::Provider(
                "Model stream ended without a final response; partial output was discarded".to_string(),
            );
            self.record_failed_metrics(&err).await?;
            return Err(err);
        };
        if let Err(err) = self.check_memory_error() {
            self.record_failed_metrics(&err).await?;
            return Err(err);
        }
        self.promote_continuation_checkpoint(response.messages.clone(), checkpoint)
            .await?;
        self.finish_turn(TurnArtifacts {
            response,
            tool_calls_count: state.total_tool_calls,
            completed_tools: sink.completed(),
            generation_elapsed_ms: elapsed,
        })
        .await
    }
}
