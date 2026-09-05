mod completion;
mod streaming_tool;
mod tool_hook;
pub mod types;

pub use types::{
    ActiveModelSwitch, CancellationSignal, QUEUED_MESSAGE_BOUNDARY, QueuedMessageBoundary, RunStatus,
    SharedModelSwitch, SteeringQueueProvider, TurnOutput, TurnRequest, UsageDetails,
};

use crate::engine::AgentEngine;
use crate::engine::runtime::build_runner;
use crate::plugin::daemon::DaemonHook;
use crate::repeat::RepeatedCallHook;
use futures::StreamExt;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::session::SessionEventKind;
use rig::agent::MultiTurnStreamItem;
use rig::memory::ConversationMemory;
use rig::streaming::StreamedAssistantContent;
use rig::tool::ToolContext;
use std::collections::HashSet;
use std::time::Instant;

use super::history::{budget_history, checkpoint_messages, continuation_history, display_events, map_streaming_error};
use super::sink::{TerminalApprovalSink, TerminalSinkConfig, TurnArtifacts};
use streaming_tool::StreamingToolTracker;
use tool_hook::TurnToolExecutionHook;

impl AgentEngine {
    pub async fn run_turn(
        &self,
        request: TurnRequest<'_>,
        presenter: std::sync::Arc<dyn Presenter>,
    ) -> Result<TurnOutput> {
        let augmented_prompt = request.prompt.to_string();
        let context = self.project_context().await?;
        self.session_manager
            .append_event(
                SessionEventKind::UserMessage,
                serde_json::json!({ "prompt": request.prompt }),
            )
            .await?;
        let preamble = context.build_system_prompt();
        let mut visible_history = ConversationMemory::load(&self.session_manager, &self.session_manager.session_id)
            .await
            .map_err(|e| AppError::Session(format!("Model-visible session history could not be loaded: {e}")))?;
        let mut checkpoint = self.session_manager.load_checkpoint().await?;

        let context_window = self
            .context_limit()
            .unwrap_or_else(|| rho_harness_core::tokens::context_window_size(&self.config.model));
        let mut history_tokens =
            rho_harness_core::tokens::calculate_context_tokens(&visible_history, None, &self.config.model).total_tokens;

        if rho_harness_core::tokens::should_compact(history_tokens, context_window, self.config.reserve_tokens) {
            let compaction_spinner = presenter.start_spinner("Compacting...");
            match self.compact_session(None).await {
                Ok(stats) => {
                    compaction_spinner.finish_and_clear();
                    presenter.print_notice(&format!(
                        "[Auto-compacted context: {} -> {} tokens (saved {})]",
                        stats.tokens_before, stats.tokens_after, stats.saved_tokens
                    ));
                    visible_history = ConversationMemory::load(&self.session_manager, &self.session_manager.session_id)
                        .await
                        .map_err(|e| {
                            AppError::Session(format!("Model-visible session history could not be loaded: {e}"))
                        })?;
                    history_tokens =
                        rho_harness_core::tokens::calculate_context_tokens(&visible_history, None, &self.config.model)
                            .total_tokens;
                }
                Err(err) => {
                    compaction_spinner.finish_and_clear();
                    eprintln!("Warning: Proactive auto-compaction failed: {err}");
                }
            }
        }

        self.run_tracker.start();
        let preamble_tokens = rho_harness_core::tokens::estimate_text_tokens(&preamble, &self.config.model);
        let prompt_tokens = rho_harness_core::tokens::estimate_text_tokens(&augmented_prompt, &self.config.model);
        let estimated_prompt_tokens = preamble_tokens
            .saturating_add(history_tokens)
            .saturating_add(prompt_tokens) as u64;
        self.usage.start_turn(Some(estimated_prompt_tokens));
        let _in_flight_guard = self.usage.in_flight_guard();
        let model_label = format!("{}:{}", self.config.model, self.context_usage_display());
        let sink = TerminalApprovalSink::new(
            &presenter,
            TerminalSinkConfig {
                model_label,
                run_tracker: self.run_tracker.clone(),
            },
            self.session_manager.clone(),
        );
        let mut current_prompt = augmented_prompt;
        let mut total_tool_calls = 0;
        let mut current_budget = self.config.max_turns;
        let mut overflow_recovered = false;

        loop {
            let mut tool_context = ToolContext::new();
            tool_context.insert(presenter.stream_port());
            let plugin_hook = DaemonHook::new(&self.config.plugins, &std::env::current_dir()?, presenter.clone()).await;
            plugin_hook.notify_turn_start(&current_prompt).await;

            let mut hook_stack = rig::agent::hook::HookStack::new();
            hook_stack.push(RepeatedCallHook::new(std::env::current_dir()?));
            hook_stack.push(plugin_hook);
            for p in &self.plugins {
                p.register_hooks(&mut hook_stack);
            }
            hook_stack.push(
                TurnToolExecutionHook::new(sink.clone(), &self.config.provider, request.steering.clone())
                    .with_model_switch(request.model_switch.clone())
                    .with_project_context(self.project_context.clone()),
            );

            let agent_guard = self.agent.read().await;
            let runner = build_runner(&agent_guard, &current_prompt)
                .conversation(self.session_manager.session_id.clone())
                .preamble(&preamble)
                .max_turns(current_budget)
                .tool_context(tool_context)
                .add_hook(hook_stack);
            drop(agent_guard);
            let runner = match checkpoint.as_ref() {
                Some(pending) => runner.history(continuation_history(&visible_history, pending)),
                None => runner,
            };
            let mut model_call_start = Some(Instant::now());
            let mut total_generation_elapsed_ms: u64 = 0;
            let mut stream = runner.stream().await;
            let mut final_response = None;
            let mut reasoning_parts = HashSet::new();
            let mut budget_hit = false;
            let mut overflow_retry = false;
            let mut streaming_tool = StreamingToolTracker::default();

            while let Some(item) = stream.next().await {
                let item = match item {
                    Ok(item) => item,
                    Err(error) => {
                        sink.finish_spinner();
                        sink.flush_display();
                        if !overflow_recovered && crate::engine::compactor::is_context_overflow_error(&error) {
                            overflow_recovered = true;
                            presenter.print_notice("[Context overflow detected: auto-compacting and retrying turn...]");
                            let compaction_spinner = presenter.start_spinner("Compacting...");
                            match self.compact_session(None).await {
                                Ok(stats) => {
                                    compaction_spinner.finish_and_clear();
                                    presenter.print_notice(&format!(
                                        "[Compacted context: {} -> {} tokens (saved {})]",
                                        stats.tokens_before, stats.tokens_after, stats.saved_tokens
                                    ));
                                    visible_history = ConversationMemory::load(
                                        &self.session_manager,
                                        &self.session_manager.session_id,
                                    )
                                    .await
                                    .map_err(|e| {
                                        AppError::Session(format!(
                                            "Model-visible session history could not be loaded: {e}"
                                        ))
                                    })?;
                                    checkpoint = self.session_manager.load_checkpoint().await?;
                                    sink.resume_model_spinner();
                                    overflow_retry = true;
                                    break;
                                }
                                Err(compact_err) => {
                                    compaction_spinner.finish_and_clear();
                                    eprintln!("Warning: Auto-compaction after context overflow failed: {compact_err}");
                                }
                            }
                        }
                        if let Some(memory_error) = self.session_manager.take_memory_error() {
                            let error = AppError::Session(memory_error);
                            self.record_failed_metrics(&error).await?;
                            return Err(error);
                        }
                        if let Some((max_turns, history)) = budget_history(&error) {
                            let pending = checkpoint_messages(&visible_history, &history)?;
                            self.session_manager.save_checkpoint(pending.clone()).await?;
                            checkpoint = Some(pending);
                            if presenter.prompt_continue_budget(max_turns).await {
                                budget_hit = true;
                                break;
                            }
                        }
                        let error = map_streaming_error(error);
                        if matches!(error, AppError::InvalidToolCall(_)) {
                            self.run_tracker.invalid_tool();
                        }
                        self.record_failed_metrics(&error).await?;
                        return Err(error);
                    }
                };
                match item {
                    MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::ToolCallDelta {
                        content,
                        ..
                    }) => {
                        sink.resume_model_spinner();
                        streaming_tool.handle_delta(content, &sink);
                    }
                    MultiTurnStreamItem::StreamAssistantItem(item) => {
                        for event in display_events(item, &mut reasoning_parts) {
                            match event {
                                super::history::DisplayEvent::Text(text) => {
                                    if model_call_start.is_none() {
                                        model_call_start = Some(Instant::now());
                                    }
                                    let active_model = request
                                        .model_switch
                                        .as_ref()
                                        .and_then(|s| s.current_model())
                                        .unwrap_or_else(|| self.config.model.clone());
                                    let delta_tokens =
                                        rho_harness_core::tokens::estimate_text_tokens(&text, &active_model) as u64;
                                    self.usage.record_streaming_chunk(delta_tokens);
                                    sink.emit_text(&text);
                                }
                                super::history::DisplayEvent::Reasoning(text) => {
                                    if model_call_start.is_none() {
                                        model_call_start = Some(Instant::now());
                                    }
                                    let active_model = request
                                        .model_switch
                                        .as_ref()
                                        .and_then(|s| s.current_model())
                                        .unwrap_or_else(|| self.config.model.clone());
                                    let delta_tokens =
                                        rho_harness_core::tokens::estimate_text_tokens(&text, &active_model) as u64;
                                    self.usage.record_streaming_chunk(delta_tokens);
                                    sink.emit_reasoning(&text);
                                }
                                super::history::DisplayEvent::ToolCall { .. } => {
                                    sink.resume_model_spinner();
                                    total_tool_calls += 1;
                                }
                            }
                        }
                    }
                    MultiTurnStreamItem::FinalResponse(response) => {
                        streaming_tool.reset();
                        final_response = Some(response);
                    }
                    MultiTurnStreamItem::CompletionCall(call) => {
                        streaming_tool.reset();
                        let elapsed_ms = if let Some(start) = model_call_start.take() {
                            let ms = start.elapsed().as_millis().max(1) as u64;
                            total_generation_elapsed_ms += ms;
                            ms
                        } else {
                            0
                        };
                        self.usage.record_step(call.usage.into(), elapsed_ms);
                        self.run_tracker.completion(call);
                    }
                    MultiTurnStreamItem::ModelTurnRetried { .. } => {
                        model_call_start = Some(Instant::now());
                        sink.resume_model_spinner();
                    }
                    MultiTurnStreamItem::ToolExecutionCommitted { .. } => {
                        streaming_tool.reset();
                        model_call_start = Some(Instant::now());
                    }
                    MultiTurnStreamItem::StreamUserItem(_) => {}
                }
            }

            if overflow_retry {
                continue;
            }

            if budget_hit {
                sink.resume_model_spinner();
                current_prompt = "Please continue where you left off and finish the task.".to_string();
                current_budget = 50;
                continue;
            }

            let generation_elapsed_ms = (total_generation_elapsed_ms
                + model_call_start.map(|s| s.elapsed().as_millis() as u64).unwrap_or(0))
            .max(1);
            sink.finish_spinner();
            sink.flush_display();
            let Some(response) = final_response else {
                let error = AppError::Provider(
                    "Model stream ended without a final response; partial output was discarded".to_string(),
                );
                self.record_failed_metrics(&error).await?;
                return Err(error);
            };
            if let Some(memory_error) = self.session_manager.take_memory_error() {
                let error = AppError::Session(memory_error);
                self.record_failed_metrics(&error).await?;
                return Err(error);
            }
            if checkpoint.is_some() {
                let messages = response.messages.clone().ok_or_else(|| {
                    AppError::Session("Completed continuation did not return canonical messages".to_string())
                })?;
                self.session_manager.promote_checkpoint(messages).await?;
            }
            let output = self
                .finish_turn(TurnArtifacts {
                    response,
                    tool_calls_count: total_tool_calls,
                    completed_tools: sink.completed(),
                    generation_elapsed_ms,
                })
                .await?;
            return Ok(output);
        }
    }
}
