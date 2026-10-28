use std::sync::Arc;
use std::time::Instant;

use futures::StreamExt;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::model::ChatMessage;
use rho_harness_core::presentation::presenter::Presenter;

use super::prepare::TurnLoopState;
use super::runner::{AgentRunner, MultiTurnStreamItem, StreamEvent};
use super::streaming_tool::StreamingToolTracker;
use crate::engine::AgentEngine;
use crate::engine::metrics::types::{CompletionCall, PromptResponse};
use crate::engine::runner::history::{
    DisplayEvent, extract_retry_after, is_content_filter_error, is_transient_network_error,
};
use crate::engine::runner::sink::TerminalApprovalSink;
use crate::engine::tracking::UsageTracker;

pub(super) struct TurnStreamState {
    pub(super) model_call_start: Option<Instant>,
    pub(super) total_generation_elapsed_ms: u64,
    pub(super) final_response: Option<PromptResponse>,
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
            streaming_tool: StreamingToolTracker::default(),
            total_tool_calls: 0,
            content_emitted: false,
        }
    }
}

pub(super) enum StreamRunResult {
    Compacted,
    RateLimitRetry,
    NetworkRetry,
    Complete(Box<TurnStreamState>),
    ContentFiltered(Box<TurnStreamState>),
}

enum StreamErrorAction {
    Compacted,
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
    usage: &UsageTracker,
    run_tracker: &crate::engine::metrics::RunTracker,
    start: &mut Option<Instant>,
    total_elapsed: &mut u64,
) {
    let elapsed_ms = if let Some(s) = start.take() {
        let ms = s.elapsed().as_millis().max(1) as u64;
        *total_elapsed += ms;
        ms
    } else {
        0
    };
    usage.record_step(call.usage, elapsed_ms);
    run_tracker.completion(call.clone());
}

fn handle_completion_stream_item(
    item: MultiTurnStreamItem,
    usage: &UsageTracker,
    run_tracker: &crate::engine::metrics::RunTracker,
    sink: &Arc<TerminalApprovalSink>,
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
                usage,
                run_tracker,
                &mut state.model_call_start,
                &mut state.total_generation_elapsed_ms,
            );
        }
        MultiTurnStreamItem::ToolCall { tool_call } => {
            state.total_tool_calls += 1;
            state.streaming_tool.handle_name(tool_call.function.name.to_string());
        }
        MultiTurnStreamItem::ModelTurnRetried => {
            state.model_call_start = Some(Instant::now());
            sink.resume_model_spinner();
        }
        MultiTurnStreamItem::ToolExecutionCommitted => {
            state.streaming_tool.reset();
            state.model_call_start = Some(Instant::now());
        }
        _ => {}
    }
}

fn print_overflow_compaction_notices(presenter: &dyn Presenter, stats: &crate::engine::CompactionStats) {
    presenter.print_notice("[Context overflow detected]");
    presenter.print_notice(&format!(
        "[Compacted context: {} -> {} tokens (saved {})]",
        stats.tokens_before, stats.tokens_after, stats.saved_tokens
    ));
    presenter.print_notice("Context was compacted after overflow. Review context and re-submit prompt.");
}

impl AgentEngine {
    fn process_assistant_stream_item(
        &self,
        event: StreamEvent,
        sink: &Arc<TerminalApprovalSink>,
        state: &mut TurnStreamState,
        active_model: &str,
    ) {
        state.content_emitted = true;
        match event {
            StreamEvent::Arguments { ref json, .. } => {
                sink.flush_reasoning();
                sink.resume_model_spinner();
                state.streaming_tool.handle_delta(json, sink);
            }
            StreamEvent::Text(t) => {
                let events = vec![DisplayEvent::Text(t)];
                handle_display_events(
                    events,
                    sink,
                    &self.usage,
                    active_model,
                    &mut state.model_call_start,
                    &mut state.total_tool_calls,
                );
            }
            StreamEvent::Reasoning(r) => {
                let events = vec![DisplayEvent::Reasoning(r)];
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
    }

    fn process_stream_item(
        &self,
        item: MultiTurnStreamItem,
        sink: &Arc<TerminalApprovalSink>,
        state: &mut TurnStreamState,
        active_model: &str,
    ) {
        if let MultiTurnStreamItem::StreamAssistantItem(content) = item {
            self.process_assistant_stream_item(content, sink, state, active_model);
        } else {
            handle_completion_stream_item(item, &self.usage, &self.run_tracker, sink, state);
        }
    }

    async fn try_recover_overflow(
        &self,
        presenter: &dyn Presenter,
        visible_history: &mut Vec<ChatMessage>,
        checkpoint: &mut Option<Vec<ChatMessage>>,
    ) -> Result<bool> {
        let _spinner = presenter.start_spinner("Compacting...");
        match self.compact_session(None).await {
            Ok(stats) if !stats.summary.is_empty() => {
                *checkpoint = None;
                *visible_history = self.session_manager.active_messages().await?;
                print_overflow_compaction_notices(presenter, &stats);
                Ok(true)
            }
            Ok(_) => Ok(false),
            Err(e) => {
                presenter.print_notice(&format!("[Auto-compaction failed: {e}]"));
                Ok(false)
            }
        }
    }

    async fn retry_rate_limit(
        &self,
        duration: std::time::Duration,
        presenter: &dyn Presenter,
        sink: &Arc<TerminalApprovalSink>,
        retries: &mut usize,
    ) -> Result<StreamErrorAction> {
        *retries += 1;
        let secs = duration.as_secs().max(1);
        presenter.print_notice(&format!("[Rate limit reached; retrying in {secs}s...]"));
        tokio::time::sleep(duration).await;
        sink.resume_model_spinner();
        Ok(StreamErrorAction::RateLimitRetry)
    }

    async fn retry_network(
        &self,
        _error: &AppError,
        presenter: &dyn Presenter,
        sink: &Arc<TerminalApprovalSink>,
        retries: &mut usize,
        content_emitted: bool,
    ) -> Result<StreamErrorAction> {
        if content_emitted {
            return Err(AppError::Network("Connection lost during active turn".to_string()));
        }
        *retries += 1;
        let secs = *retries as u64;
        presenter.print_notice(&format!(
            "[Network connection failed; retrying in {secs}s ({retries}/2)...]"
        ));
        tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
        sink.resume_model_spinner();
        Ok(StreamErrorAction::NetworkRetry)
    }

    async fn try_context_overflow(
        &self,
        error: &AppError,
        presenter: &dyn Presenter,
        visible_history: &mut Vec<ChatMessage>,
        checkpoint: &mut Option<Vec<ChatMessage>>,
        overflow_recovered: &mut bool,
    ) -> Result<bool> {
        if !*overflow_recovered && crate::engine::compactor::is_context_overflow_error(error) {
            *overflow_recovered = true;
            return self.try_recover_overflow(presenter, visible_history, checkpoint).await;
        }
        Ok(false)
    }

    async fn handle_fatal_stream_error(&self, error: AppError) -> Result<StreamErrorAction> {
        if matches!(error, AppError::InvalidToolCall(_)) {
            self.run_tracker.invalid_tool();
        }
        self.record_failed_metrics(&error).await?;
        Err(error)
    }

    async fn handle_stream_error(
        &self,
        error: AppError,
        presenter: &dyn Presenter,
        sink: &Arc<TerminalApprovalSink>,
        loop_state: &mut TurnLoopState,
        content_emitted: bool,
    ) -> Result<StreamErrorAction> {
        sink.finish_spinner();
        sink.flush_display();
        if self
            .try_context_overflow(
                &error,
                presenter,
                &mut loop_state.visible_history,
                &mut loop_state.checkpoint,
                &mut loop_state.overflow_recovered,
            )
            .await?
        {
            return Ok(StreamErrorAction::Compacted);
        }
        if let Some(mem_err) = self.session_manager.take_memory_error() {
            let err = AppError::Session(mem_err);
            self.record_failed_metrics(&err).await?;
            return Err(err);
        }
        if let Some(duration) = extract_retry_after(&error)
            && duration.as_secs() <= 30
            && loop_state.rate_limit_retries < 2
        {
            return self
                .retry_rate_limit(duration, presenter, sink, &mut loop_state.rate_limit_retries)
                .await;
        }
        if is_transient_network_error(&error) && !content_emitted && loop_state.network_retries < 2 {
            return self
                .retry_network(
                    &error,
                    presenter,
                    sink,
                    &mut loop_state.network_retries,
                    content_emitted,
                )
                .await;
        }
        self.handle_fatal_stream_error(error).await
    }

    pub(super) async fn run_turn_stream(
        &self,
        runner: AgentRunner,
        sink: &Arc<TerminalApprovalSink>,
        presenter: &dyn Presenter,
        active_model: &str,
        loop_state: &mut TurnLoopState,
    ) -> Result<StreamRunResult> {
        let mut state = TurnStreamState::new();
        let mut stream = runner.stream();
        while let Some(item) = stream.next().await {
            match item {
                Ok(item) => self.process_stream_item(item, sink, &mut state, active_model),
                Err(err) => {
                    if is_content_filter_error(&err) {
                        return Ok(StreamRunResult::ContentFiltered(Box::new(state)));
                    }
                    let action = self
                        .handle_stream_error(err, presenter, sink, loop_state, state.content_emitted)
                        .await?;
                    match action {
                        StreamErrorAction::Compacted => return Ok(StreamRunResult::Compacted),
                        StreamErrorAction::RateLimitRetry => return Ok(StreamRunResult::RateLimitRetry),
                        StreamErrorAction::NetworkRetry => return Ok(StreamRunResult::NetworkRetry),
                    }
                }
            }
            tokio::task::yield_now().await;
        }
        Ok(StreamRunResult::Complete(Box::new(state)))
    }

    pub(super) fn elapsed_generation_ms(state: &TurnStreamState) -> u64 {
        (state.total_generation_elapsed_ms
            + state
                .model_call_start
                .map(|s| s.elapsed().as_millis() as u64)
                .unwrap_or(0))
        .max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::eval::mock::{MockEngineConfig, mock_engine};
    use crate::provider::adapter::MockAdapter;
    use async_trait::async_trait;
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

    #[tokio::test(start_paused = true)]
    async fn test_transient_network_error_retries_then_exhausts() {
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
        let presenter: Arc<dyn Presenter> = Arc::new(TestPresenter::default());
        let sink = engine.create_approval_sink(&presenter);
        let mut loop_state = TurnLoopState {
            visible_history: Vec::new(),
            checkpoint: None,
            current_prompt: String::new(),
            current_budget: 10,
            overflow_recovered: false,
            rate_limit_retries: 0,
            network_retries: 0,
        };

        let transient_err =
            AppError::Provider("error sending request for url (https://cloudcode-pa.googleapis.com)".to_string());

        let action = engine
            .handle_stream_error(transient_err, presenter.as_ref(), &sink, &mut loop_state, false)
            .await
            .unwrap();
        assert!(matches!(action, StreamErrorAction::NetworkRetry));
        assert_eq!(loop_state.network_retries, 1);

        let transient_err = AppError::Provider("connection reset by peer".to_string());
        let action = engine
            .handle_stream_error(transient_err, presenter.as_ref(), &sink, &mut loop_state, false)
            .await
            .unwrap();
        assert!(matches!(action, StreamErrorAction::NetworkRetry));
        assert_eq!(loop_state.network_retries, 2);

        let transient_err = AppError::Provider("connection reset by peer".to_string());
        let result = engine
            .handle_stream_error(transient_err, presenter.as_ref(), &sink, &mut loop_state, false)
            .await;
        assert!(result.is_err());
        assert_eq!(loop_state.network_retries, 2);
    }

    #[tokio::test]
    async fn test_transient_network_error_not_retried_if_content_already_emitted() {
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
        let presenter: Arc<dyn Presenter> = Arc::new(TestPresenter::default());
        let sink = engine.create_approval_sink(&presenter);
        let mut loop_state = TurnLoopState {
            visible_history: Vec::new(),
            checkpoint: None,
            current_prompt: String::new(),
            current_budget: 10,
            overflow_recovered: false,
            rate_limit_retries: 0,
            network_retries: 0,
        };

        let transient_err = AppError::Provider("Claude stream failed: broken pipe".to_string());

        let result = engine
            .handle_stream_error(transient_err, presenter.as_ref(), &sink, &mut loop_state, true)
            .await;
        assert!(result.is_err());
        assert_eq!(loop_state.network_retries, 0);
    }
}
