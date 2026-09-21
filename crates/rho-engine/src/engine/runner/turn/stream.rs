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
    DisplayEvent, budget_history, checkpoint_messages, display_events, extract_retry_after, is_transient_network_error,
    map_streaming_error, streaming_error_provider_request_id,
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
    pub(super) content_emitted: bool,
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
            content_emitted: false,
        }
    }
}

pub(super) enum StreamRunResult {
    Compacted,
    BudgetContinue,
    RateLimitRetry,
    NetworkRetry,
    Complete(Box<TurnStreamState>),
}

enum StreamErrorAction {
    Compacted,
    BudgetContinue,
    RateLimitRetry,
    NetworkRetry,
}

fn record_streaming_text(text: &str, _model: &str, usage: &UsageTracker, start: &mut Option<Instant>) {
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
    model: &str,
    start: &mut Option<Instant>,
    tool_calls: &mut usize,
) {
    for event in events {
        match event {
            DisplayEvent::Text(text) => {
                record_streaming_text(&text, model, usage, start);
                sink.emit_text(&text);
            }
            DisplayEvent::Reasoning(text) => {
                record_streaming_text(&text, model, usage, start);
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

fn print_overflow_compaction_notices(presenter: &dyn Presenter, stats: &crate::engine::CompactionStats) {
    presenter.print_notice(&format!(
        "[Compacted context: {} -> {} tokens (saved {})]",
        stats.tokens_before, stats.tokens_after, stats.saved_tokens
    ));
    presenter.print_notice("Context was compacted after overflow. Review context and re-submit prompt.");
}

impl AgentEngine {
    fn process_assistant_stream_item(
        &self,
        content: StreamedAssistantContent,
        (sink, state, active_model): (&Arc<TerminalApprovalSink>, &mut TurnStreamState, &str),
    ) {
        state.content_emitted = true;
        if let StreamedAssistantContent::ToolCallDelta { content, .. } = content {
            sink.flush_reasoning();
            sink.resume_model_spinner();
            state.streaming_tool.handle_delta(content, sink);
        } else {
            let events = display_events(content, &mut state.reasoning_parts);
            handle_display_events(
                events,
                sink,
                &self.usage,
                active_model,
                &mut state.model_call_start,
                &mut state.total_tool_calls,
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
        presenter: &dyn Presenter,
        (visible_history, checkpoint): (&mut Vec<Message>, &mut Option<Vec<Message>>),
    ) -> Result<bool> {
        presenter.print_notice("[Context overflow detected: auto-compacting...]");
        let spinner = presenter.start_spinner("Compacting...");
        let stats = match self.compact_session(None).await {
            Ok(s) if !s.summary.is_empty() => s,
            Ok(_) => {
                spinner.finish_and_clear();
                return Ok(false);
            }
            Err(e) => {
                spinner.finish_and_clear();
                eprintln!("Warning: Auto-compaction after context overflow failed: {e}");
                return Ok(false);
            }
        };
        spinner.finish_and_clear();
        print_overflow_compaction_notices(presenter, &stats);
        *visible_history = ConversationMemory::load(&self.session_manager, &self.session_manager.session_id)
            .await
            .map_err(|e| AppError::Session(format!("Model-visible session history could not be loaded: {e}")))?;
        *checkpoint = self.session_manager.load_checkpoint().await?;
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
        (error, presenter): (&StreamingError, &dyn Presenter),
        (visible_history, checkpoint, overflow_recovered): (&mut Vec<Message>, &mut Option<Vec<Message>>, &mut bool),
    ) -> Result<bool> {
        if !*overflow_recovered && crate::engine::compactor::is_context_overflow_error(error) {
            *overflow_recovered = true;
            return self
                .try_recover_overflow(presenter, (visible_history, checkpoint))
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
        if let Some(req_id) = streaming_error_provider_request_id(&error) {
            self.run_tracker.set_last_request_id(req_id);
        }
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
        (visible_history, checkpoint, overflow_recovered, rate_limit_retries, network_retries, content_emitted): (
            &mut Vec<Message>,
            &mut Option<Vec<Message>>,
            &mut bool,
            &mut usize,
            &mut usize,
            bool,
        ),
    ) -> Result<StreamErrorAction> {
        sink.finish_spinner();
        sink.flush_display();
        if self
            .try_context_overflow((&error, presenter), (visible_history, checkpoint, overflow_recovered))
            .await?
        {
            return Ok(StreamErrorAction::Compacted);
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
        if let Some(duration) = extract_retry_after(&error)
            && duration.as_secs() <= 30
            && *rate_limit_retries < 2
        {
            *rate_limit_retries += 1;
            let secs = duration.as_secs().max(1);
            presenter.print_notice(&format!("[Rate limit reached; retrying in {secs}s...]"));
            tokio::time::sleep(duration).await;
            sink.resume_model_spinner();
            return Ok(StreamErrorAction::RateLimitRetry);
        }
        if is_transient_network_error(&error) && !content_emitted && *network_retries < 2 {
            *network_retries += 1;
            let secs = *network_retries as u64;
            presenter.print_notice(&format!(
                "[Network connection failed; retrying in {secs}s ({}/2)...]",
                *network_retries
            ));
            tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
            sink.resume_model_spinner();
            return Ok(StreamErrorAction::NetworkRetry);
        }
        self.handle_fatal_stream_error(error).await
    }

    pub(super) async fn run_turn_stream(
        &self,
        (runner, sink, presenter): (AgentRunner, &Arc<TerminalApprovalSink>, &dyn Presenter),
        (active_model, visible_history, checkpoint, overflow_recovered, rate_limit_retries, network_retries): (
            &str,
            &mut Vec<Message>,
            &mut Option<Vec<Message>>,
            &mut bool,
            &mut usize,
            &mut usize,
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
                            (
                                visible_history,
                                checkpoint,
                                overflow_recovered,
                                rate_limit_retries,
                                network_retries,
                                state.content_emitted,
                            ),
                        )
                        .await?;
                    match action {
                        StreamErrorAction::Compacted => return Ok(StreamRunResult::Compacted),
                        StreamErrorAction::BudgetContinue => return Ok(StreamRunResult::BudgetContinue),
                        StreamErrorAction::RateLimitRetry => return Ok(StreamRunResult::RateLimitRetry),
                        StreamErrorAction::NetworkRetry => return Ok(StreamRunResult::NetworkRetry),
                    }
                }
            }
            tokio::task::yield_now().await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::eval::mock::{MockEngineConfig, mock_engine};
    use async_trait::async_trait;
    use rig::completion::CompletionError;
    use rig::test_utils::MockCompletionModel;
    use std::sync::Mutex;

    #[derive(Default)]
    struct TestPresenter {
        notices: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl Presenter for TestPresenter {
        fn write_output(&self, _text: &str) {}
        fn print_welcome(&self, _display: &rho_harness_core::presentation::WelcomeDisplay) {}
        fn print_session_status(&self, _display: &rho_harness_core::presentation::SessionStatus) {}
        fn print_notice(&self, text: &str) {
            self.notices.lock().unwrap().push(text.to_string());
        }
        fn print_user_block(&self, _input: &str) {}
        fn print_token(&self, _token: &str) {}
        fn print_thinking_token(&self, _token: &str) {}
        fn finish_tool_line(&self, _line: rho_harness_core::presentation::ToolLine) {}
        fn flush(&self) {}
        fn has_interactive_ui(&self) -> bool {
            false
        }
        fn start_spinner(&self, _message: &str) -> rho_harness_core::presentation::activity::ActivityToken {
            rho_harness_core::presentation::activity::ActivityToken::default()
        }
        fn start_tool_spinner(
            &self,
            _name: &str,
            _arguments: &serde_json::Value,
        ) -> rho_harness_core::presentation::activity::ActivityToken {
            rho_harness_core::presentation::activity::ActivityToken::default()
        }
        fn start_tool_run(&self, _name: &str, _arguments: &serde_json::Value) {}
        fn stream_port(&self) -> rho_harness_core::presentation::ToolStreamPort {
            rho_harness_core::presentation::ToolStreamPort::default()
        }
    }

    #[tokio::test]
    async fn test_transient_network_error_retries_then_exhausts() {
        let temp_dir = tempfile::tempdir().unwrap();
        let engine = mock_engine(
            MockCompletionModel::default(),
            MockEngineConfig {
                base_dir: temp_dir.path(),
                app_config: rho_harness_core::config::Config::default(),
                session_manager: None,
                built_in_tools: None,
            },
        );
        let presenter: Arc<dyn Presenter> = Arc::new(TestPresenter::default());
        let sink = engine.create_approval_sink(&presenter);
        let mut visible_history = Vec::new();
        let mut checkpoint = None;
        let mut overflow_recovered = false;
        let mut rate_limit_retries = 0;
        let mut network_retries = 0;

        let transient_err = StreamingError::Completion(CompletionError::ProviderError(
            "error sending request for url (https://cloudcode-pa.googleapis.com)".to_string(),
        ));

        let action = engine
            .handle_stream_error(
                (transient_err, presenter.as_ref(), &sink),
                (
                    &mut visible_history,
                    &mut checkpoint,
                    &mut overflow_recovered,
                    &mut rate_limit_retries,
                    &mut network_retries,
                    false,
                ),
            )
            .await
            .unwrap();
        assert!(matches!(action, StreamErrorAction::NetworkRetry));
        assert_eq!(network_retries, 1);

        let transient_err =
            StreamingError::Completion(CompletionError::ProviderError("connection reset by peer".to_string()));
        let action = engine
            .handle_stream_error(
                (transient_err, presenter.as_ref(), &sink),
                (
                    &mut visible_history,
                    &mut checkpoint,
                    &mut overflow_recovered,
                    &mut rate_limit_retries,
                    &mut network_retries,
                    false,
                ),
            )
            .await
            .unwrap();
        assert!(matches!(action, StreamErrorAction::NetworkRetry));
        assert_eq!(network_retries, 2);

        let transient_err =
            StreamingError::Completion(CompletionError::ProviderError("connection reset by peer".to_string()));
        let result = engine
            .handle_stream_error(
                (transient_err, presenter.as_ref(), &sink),
                (
                    &mut visible_history,
                    &mut checkpoint,
                    &mut overflow_recovered,
                    &mut rate_limit_retries,
                    &mut network_retries,
                    false,
                ),
            )
            .await;
        assert!(result.is_err());
        assert_eq!(network_retries, 2);
    }

    #[tokio::test]
    async fn test_transient_network_error_not_retried_if_content_already_emitted() {
        let temp_dir = tempfile::tempdir().unwrap();
        let engine = mock_engine(
            MockCompletionModel::default(),
            MockEngineConfig {
                base_dir: temp_dir.path(),
                app_config: rho_harness_core::config::Config::default(),
                session_manager: None,
                built_in_tools: None,
            },
        );
        let presenter: Arc<dyn Presenter> = Arc::new(TestPresenter::default());
        let sink = engine.create_approval_sink(&presenter);
        let mut visible_history = Vec::new();
        let mut checkpoint = None;
        let mut overflow_recovered = false;
        let mut rate_limit_retries = 0;
        let mut network_retries = 0;

        let transient_err = StreamingError::Completion(CompletionError::ProviderError(
            "Claude stream failed: broken pipe".to_string(),
        ));

        let result = engine
            .handle_stream_error(
                (transient_err, presenter.as_ref(), &sink),
                (
                    &mut visible_history,
                    &mut checkpoint,
                    &mut overflow_recovered,
                    &mut rate_limit_retries,
                    &mut network_retries,
                    true,
                ),
            )
            .await;
        assert!(result.is_err());
        assert_eq!(network_retries, 0);
    }
}
