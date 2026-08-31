pub mod types;

pub use types::{QUEUED_MESSAGE_BOUNDARY, QueuedMessageBoundary, RunStatus, TurnOutput, TurnRequest, UsageDetails};

use crate::engine::AgentEngine;
use crate::engine::metrics::{RunMetrics, TerminalStatus};
use crate::engine::runtime::build_runner;
use crate::repeat::RepeatedCallHook;
use futures::StreamExt;
use rho_core::approval::{ApprovalCapability, approval_context};
use rho_core::error::{AppError, Result};
use rho_core::presentation::presenter::Presenter;
use rho_core::session::SessionEventKind;
use rho_core::session::context::context_memory;
use rig::agent::MultiTurnStreamItem;
use rig::completion::FinishReason;
use rig::memory::ConversationMemory;
use rig::streaming::StreamedAssistantContent;
use std::collections::HashSet;
use std::time::Instant;

use super::helpers::redact_text;
use super::history::{budget_history, checkpoint_messages, continuation_history, display_events, map_streaming_error};
use super::sink::{TerminalApprovalSink, TerminalSinkConfig, TurnArtifacts};

impl AgentEngine {
    pub async fn run_turn(
        &self,
        request: TurnRequest<'_>,
        presenter: std::sync::Arc<dyn Presenter>,
    ) -> Result<TurnOutput> {
        self.notify_before_turn(request.prompt).await;
        let result = match &self.backend {
            crate::engine::AgentBackend::Rig(_) => self.run_rig_turn(request, presenter).await,
            crate::engine::AgentBackend::External { .. } => self.run_external_turn(request, presenter).await,
        };
        self.notify_after_turn(result.is_ok(), Vec::new()).await;
        result
    }

    async fn run_rig_turn(
        &self,
        request: TurnRequest<'_>,
        presenter: std::sync::Arc<dyn Presenter>,
    ) -> Result<TurnOutput> {
        let augmented_prompt = self.augment_prompt_with_context(request.prompt, &presenter).await;
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
        .map_err(|_| AppError::Session("Model-visible session history could not be loaded".to_string()))?;
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
        let capability = ApprovalCapability::with_session_grants(
            self.config.auto_approve,
            sink.clone(),
            self.session_approvals.clone(),
        );
        let mut current_prompt = augmented_prompt;
        let mut total_tool_calls = 0;
        let mut current_budget = self.config.max_turns;

        loop {
            let mut tool_context = approval_context(capability.clone());
            tool_context.insert(presenter.question_port());
            tool_context.insert(presenter.stream_port());
            tool_context.insert(rho_sdk::contract::InvocationContext {
                session_id: self.session_manager.session_id.clone(),
                working_directory: std::env::current_dir()?.display().to_string(),
                has_interactive_ui: presenter.has_interactive_ui(),
                plugin_config: None,
            });
            let crate::engine::AgentBackend::Rig(agent) = &self.backend else {
                return Err(AppError::Provider("internal provider runtime mismatch".to_string()));
            };
            let runner = build_runner(agent, &current_prompt)
                .conversation(self.session_manager.session_id.clone())
                .preamble(&preamble)
                .max_turns(current_budget)
                .tool_context(tool_context)
                .add_hook(RepeatedCallHook::new(std::env::current_dir()?).with_sink(sink.clone()));
            let runner = match checkpoint.as_ref() {
                Some(pending) => runner.history(continuation_history(&visible_history, pending)),
                None => runner,
            };
            let stream_start = Instant::now();
            let mut stream = runner.stream().await;
            let mut final_response = None;
            let mut reasoning_parts = HashSet::new();
            let mut budget_hit = false;

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
                    MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::ToolCallDelta { .. }) => {
                        sink.resume_model_spinner();
                    }
                    MultiTurnStreamItem::StreamAssistantItem(item) => {
                        for event in display_events(item, &mut reasoning_parts) {
                            match event {
                                super::history::DisplayEvent::Text(text) => sink.emit_text(&text),
                                super::history::DisplayEvent::Reasoning(text) => sink.emit_reasoning(&text),
                                super::history::DisplayEvent::ToolCall => {
                                    sink.resume_model_spinner();
                                    total_tool_calls += 1;
                                }
                            }
                        }
                    }
                    MultiTurnStreamItem::FinalResponse(response) => final_response = Some(response),
                    MultiTurnStreamItem::CompletionCall(call) => self.run_tracker.completion(call),
                    MultiTurnStreamItem::ModelTurnRetried { .. } => sink.resume_model_spinner(),
                    MultiTurnStreamItem::StreamUserItem(_) | MultiTurnStreamItem::ToolExecutionCommitted { .. } => {}
                }
            }

            if budget_hit {
                sink.resume_model_spinner();
                current_prompt = "Please continue where you left off and finish the task.".to_string();
                current_budget = 50;
                continue;
            }

            let generation_elapsed_ms = stream_start.elapsed().as_millis().max(1) as u64;
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

    pub async fn record_cancellation(&self, reason: &str) -> Result<()> {
        self.session_manager
            .append_event(
                SessionEventKind::Cancellation,
                serde_json::json!({ "reason": redact_text(reason), "terminal": true }),
            )
            .await?;
        let metrics = self
            .run_tracker
            .terminate(&self.session_manager.session_id, TerminalStatus::Cancelled);
        self.record_run_summary(&metrics).await
    }

    pub(super) async fn record_failed_metrics(&self, error: &AppError) -> Result<()> {
        let status = if matches!(error, AppError::ModelBudgetExhausted { .. }) {
            TerminalStatus::BudgetExhausted
        } else if matches!(error, AppError::Cancelled(_)) {
            TerminalStatus::Cancelled
        } else {
            TerminalStatus::Failed
        };
        let metrics = self.run_tracker.terminate(&self.session_manager.session_id, status);
        self.record_run_summary(&metrics).await
    }

    pub(super) async fn record_run_summary(&self, metrics: &RunMetrics) -> Result<()> {
        self.session_manager
            .append_event(
                SessionEventKind::RunSummary,
                serde_json::to_value(metrics).map_err(|error| AppError::Other(error.into()))?,
            )
            .await
    }

    async fn finish_turn(&self, artifacts: TurnArtifacts) -> Result<TurnOutput> {
        let TurnArtifacts {
            response,
            tool_calls_count,
            completed_tools,
            generation_elapsed_ms,
        } = artifacts;

        for tool in &completed_tools {
            self.session_manager
                .append_event(
                    SessionEventKind::ToolCall,
                    serde_json::json!({
                        "id": tool.internal_call_id,
                        "name": tool.name,
                        "arguments": tool.arguments,
                    }),
                )
                .await?;
            self.session_manager
                .append_event(
                    SessionEventKind::ToolResult,
                    serde_json::json!({
                        "id": tool.internal_call_id,
                        "name": tool.name,
                        "output": tool.output,
                        "status": tool.status,
                    }),
                )
                .await?;
        }
        self.session_manager
            .append_event(
                SessionEventKind::AssistantResponse,
                serde_json::json!({ "content": response.output }),
            )
            .await?;

        let usage = response.usage;
        let usage_details = usage.has_values().then(|| usage.into());
        if generation_elapsed_ms > 0 {
            self.usage.record_with_duration(usage.into(), generation_elapsed_ms);
        } else {
            self.record_usage(usage.into());
        }
        self.session_manager
            .append_event(
                SessionEventKind::UsageMetrics,
                serde_json::json!({ "available": usage_details.is_some(), "usage": usage_details }),
            )
            .await?;
        let status = if response
            .completion_calls
            .last()
            .and_then(|call| call.finish_reason.as_ref())
            == Some(&FinishReason::ContentFilter)
        {
            RunStatus::ContentFiltered
        } else {
            RunStatus::Completed
        };

        let requests = response.requests();
        let terminal_status = match status {
            RunStatus::Completed => TerminalStatus::Completed,
            RunStatus::ContentFiltered => TerminalStatus::ContentFiltered,
        };
        let metrics = self.run_tracker.complete(crate::engine::metrics::CompletionOutcome {
            session_id: &self.session_manager.session_id,
            status: terminal_status,
            response: &response,
        });
        self.record_run_summary(&metrics).await?;
        Ok(TurnOutput {
            final_text: response.output,
            tool_calls_count,
            tool_failures_count: completed_tools.iter().filter(|tool| tool.status != "success").count(),
            requests,
            usage: usage_details,
            status,
            metrics,
        })
    }
}
