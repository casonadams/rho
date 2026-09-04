mod completion;
mod streaming_tool;
mod tool_hook;
pub mod types;

pub use types::{
    CancellationSignal, QUEUED_MESSAGE_BOUNDARY, QueuedMessageBoundary, RunStatus, SteeringQueueProvider, TurnOutput,
    TurnRequest, UsageDetails,
};

use crate::engine::AgentEngine;
use crate::engine::runtime::build_runner;
use crate::plugin::daemon::DaemonHook;
use crate::repeat::RepeatedCallHook;
use futures::StreamExt;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::session::SessionEventKind;
use rho_harness_core::session::context::context_memory;
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
        self.ensure_tools_loaded().await?;
        let augmented_prompt = request.prompt.to_string();
        let context = self.project_context().await?;
        self.session_manager
            .append_event(
                SessionEventKind::UserMessage,
                serde_json::json!({ "prompt": request.prompt }),
            )
            .await?;
        let preamble = context.build_system_prompt();
        let visible_history = context_memory(
            self.session_manager.clone(),
            self.config.context_window_messages,
            self.config.compaction_max_bytes,
        )
        .load(&self.session_manager.session_id)
        .await
        .map_err(|e| AppError::Session(format!("Model-visible session history could not be loaded: {e}")))?;
        let mut checkpoint = self.session_manager.load_checkpoint().await?;

        self.run_tracker.start();
        self.usage.start_response();
        let model_label = format!("{}:{}", self.config.model, self.context_usage_display());
        let sink = TerminalApprovalSink::new(
            &presenter,
            TerminalSinkConfig {
                model_label,
                auto_approve: self.config.auto_approve,
                run_tracker: self.run_tracker.clone(),
            },
            self.session_manager.clone(),
        );
        let mut current_prompt = augmented_prompt;
        let mut total_tool_calls = 0;
        let mut current_budget = self.config.max_turns;

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
            hook_stack.push(TurnToolExecutionHook::new(sink.clone(), &self.config.provider));

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
            let mut streaming_tool = StreamingToolTracker::default();

            while let Some(item) = stream.next().await {
                let item = match item {
                    Ok(item) => item,
                    Err(error) => {
                        sink.finish_spinner();
                        sink.flush_display();
                        if let Some(memory_error) = self.session_manager.take_memory_error() {
                            let error = AppError::Session(memory_error);
                            self.record_failed_metrics(&error).await?;
                            return Err(error);
                        }
                        if let Some((max_turns, history)) = budget_history(&error) {
                            let pending = checkpoint_messages(&visible_history, &history)?;
                            self.session_manager.save_checkpoint(pending.clone()).await?;
                            checkpoint = Some(pending);
                            if !self.config.auto_approve && presenter.prompt_continue_budget(max_turns).await {
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
                        if model_call_start.is_none() {
                            model_call_start = Some(Instant::now());
                        }
                        for event in display_events(item, &mut reasoning_parts) {
                            match event {
                                super::history::DisplayEvent::Text(text) => sink.emit_text(&text),
                                super::history::DisplayEvent::Reasoning(text) => sink.emit_reasoning(&text),
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
                        if let Some(start) = model_call_start.take() {
                            total_generation_elapsed_ms += start.elapsed().as_millis().max(1) as u64;
                        }
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
