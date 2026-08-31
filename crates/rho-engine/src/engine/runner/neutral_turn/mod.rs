pub mod artifacts;
pub mod observer;

pub(crate) use artifacts::{ExternalTurnArtifacts, neutral_history_to_rig, rig_history_to_neutral, structural_usage};
pub(crate) use observer::TurnObserver;

use super::sink::{TerminalApprovalSink, TerminalSinkConfig};
use super::turn::{RunStatus, TurnOutput, TurnRequest};
use crate::engine::metrics::TerminalStatus;
use crate::engine::provider::host_loop::{
    CancellationSignal, NeutralTurnRequest, NeutralTurnRuntime, NeutralTurnTerminal, run_neutral_turn,
};
use crate::engine::{AgentBackend, AgentEngine};
use rho_core::approval::{ApprovalCapability, approval_context};
use rho_core::error::{AppError, Result};
use rho_core::presentation::presenter::Presenter;
use rho_core::session::SessionEventKind;
use rho_core::session::context::context_memory;
use rho_sdk::contract::{MessageContent, MessageRole, ModelMessage};
use std::sync::atomic::Ordering;

impl AgentEngine {
    pub(super) async fn run_external_turn(
        &self,
        request: TurnRequest<'_>,
        presenter: std::sync::Arc<dyn Presenter>,
    ) -> Result<TurnOutput> {
        let AgentBackend::External {
            provider,
            tools,
            credential,
        } = &self.backend
        else {
            return Err(AppError::Provider("external provider runtime mismatch".to_string()));
        };
        let context = self.project_context().await?;
        self.session_manager
            .append_event(
                SessionEventKind::UserMessage,
                serde_json::json!({"prompt": request.prompt}),
            )
            .await?;
        let preamble = context.build_system_prompt();

        self.run_tracker.start();
        self.usage.start_response();
        let sink = TerminalApprovalSink::new(
            &presenter,
            TerminalSinkConfig {
                model_label: format!("{}:{}", self.config.model, self.context_usage_display()),
                auto_approve: self.config.auto_approve,
                run_tracker: self.run_tracker.clone(),
            },
            self.session_manager.clone(),
        );
        let approval = ApprovalCapability::with_session_grants(
            self.config.auto_approve,
            sink.clone(),
            self.session_approvals.clone(),
        );
        let mut tool_context = approval_context(approval);
        tool_context.insert(presenter.question_port());
        tool_context.insert(presenter.stream_port());
        tool_context.insert(rho_sdk::contract::InvocationContext {
            session_id: self.session_manager.session_id.clone(),
            working_directory: std::env::current_dir()?.display().to_string(),
            has_interactive_ui: presenter.has_interactive_ui(),
            plugin_config: None,
        });
        tools.begin_turn(&mut tool_context).await;
        let observer = TurnObserver::new(sink.clone());
        let prior_history = context_memory(
            self.session_manager.clone(),
            self.config.context_window_messages,
            self.config.compaction_max_bytes,
        )
        .load(&self.session_manager.session_id)
        .await
        .map_err(|_| AppError::Session("Model-visible session history could not be loaded".to_string()))?;
        let augmented_prompt = self.augment_prompt_with_context(request.prompt, &presenter).await;
        let mut messages = vec![ModelMessage {
            role: MessageRole::System,
            content: vec![MessageContent::Text { text: preamble }],
        }];
        messages.extend(rig_history_to_neutral(&prior_history));
        let new_history_start = messages.len();
        messages.push(ModelMessage {
            role: MessageRole::User,
            content: vec![MessageContent::Text { text: augmented_prompt }],
        });
        let mut checkpoint = None;
        let mut max_turns = self.config.max_turns;

        let window_size = self
            .context_limit()
            .unwrap_or_else(|| rho_core::tokens::context_window_size(&self.config.model));
        let token_stats = rho_core::tokens::calculate_context_tokens(&messages, None, &self.config.model);
        if rho_core::tokens::should_compact(token_stats.total_tokens, window_size, self.config.reserve_tokens) {
            let cut_idx =
                rho_core::tokens::find_token_cut_point(&messages, self.config.keep_recent_tokens, &self.config.model);
            if cut_idx > 1 && cut_idx < messages.len() {
                let mut compacted = vec![messages[0].clone()];
                compacted.extend_from_slice(&messages[cut_idx..]);
                messages = compacted;
            }
        }

        let default_cancellation = CancellationSignal::default();
        let cancellation = request.cancellation.unwrap_or(&default_cancellation);
        let default_steering = crate::engine::provider::host_loop::NoopSteeringQueue;
        let steering = request.steering.unwrap_or(&default_steering);

        loop {
            let terminal = run_neutral_turn(
                NeutralTurnRuntime {
                    provider: provider.as_ref(),
                    tools: &**tools,
                    observer: &observer,
                    cancellation,
                    steering,
                },
                NeutralTurnRequest {
                    model: self.config.model.clone(),
                    messages: messages.clone(),
                    credential: credential.clone(),
                    max_output_tokens: self.config.max_output_tokens,
                    tools: tools.provider_definitions(),
                    max_turns,
                    checkpoint: checkpoint.clone(),
                },
            )
            .await
            .map_err(|error| AppError::Provider(error.to_string()))?;
            match terminal {
                NeutralTurnTerminal::Checkpoint(pending) => {
                    if !self.config.auto_approve
                        && presenter.prompt_continue_budget(pending.completed_model_turns).await
                    {
                        messages = Vec::new();
                        checkpoint = Some(pending);
                        max_turns = 50;
                        sink.resume_model_spinner();
                        continue;
                    }
                    let error = AppError::ModelBudgetExhausted {
                        max_turns: pending.completed_model_turns,
                    };
                    self.record_failed_metrics(&error).await?;
                    return Err(error);
                }
                NeutralTurnTerminal::Cancelled(_) => {
                    let error = AppError::Cancelled("provider operation cancelled".to_string());
                    self.record_failed_metrics(&error).await?;
                    return Err(error);
                }
                NeutralTurnTerminal::Completed(mut output) => {
                    sink.finish_spinner();
                    sink.flush_display();
                    let new_messages = output.messages.split_off(new_history_start);
                    self.persist_canonical_history(&new_messages).await?;
                    return self
                        .finish_external_turn(ExternalTurnArtifacts {
                            output,
                            tool_calls_count: observer.tool_calls.load(Ordering::Relaxed),
                            completed_tools: sink.completed(),
                        })
                        .await;
                }
            }
        }
    }

    async fn persist_canonical_history(&self, new_messages: &[ModelMessage]) -> Result<()> {
        let canonical = neutral_history_to_rig(new_messages);
        if canonical.is_empty() {
            return Ok(());
        }
        rho_core::session::context::context_memory(
            self.session_manager.clone(),
            self.config.context_window_messages,
            self.config.compaction_max_bytes,
        )
        .append(&self.session_manager.session_id, canonical)
        .await
        .map_err(|_| AppError::Session("External provider history could not be committed".to_string()))
    }

    async fn finish_external_turn(&self, artifacts: ExternalTurnArtifacts) -> Result<TurnOutput> {
        let ExternalTurnArtifacts {
            output,
            tool_calls_count,
            completed_tools,
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
                serde_json::json!({"content": output.text}),
            )
            .await?;
        let generation_elapsed_ms = output.usage.generation_elapsed_ms;
        let usage = structural_usage(output.usage);
        let usage = usage.has_values().then_some(usage);
        if let Some(usage) = usage {
            if generation_elapsed_ms > 0 {
                self.usage.record_with_duration(usage, generation_elapsed_ms);
            } else {
                self.record_usage(usage);
            }
        }
        self.session_manager
            .append_event(
                SessionEventKind::UsageMetrics,
                serde_json::json!({"available": usage.is_some(), "usage": usage}),
            )
            .await?;
        let status = if output.finish_reason == rho_sdk::contract::FinishReason::ContentFilter {
            RunStatus::ContentFiltered
        } else {
            RunStatus::Completed
        };
        let terminal_status = match status {
            RunStatus::Completed => TerminalStatus::Completed,
            RunStatus::ContentFiltered => TerminalStatus::ContentFiltered,
        };
        let metrics = self
            .run_tracker
            .complete_neutral(crate::engine::metrics::NeutralOutcome {
                session_id: &self.session_manager.session_id,
                status: terminal_status,
                requests: output.model_turns,
                usage,
            });
        self.record_run_summary(&metrics).await?;
        Ok(TurnOutput {
            final_text: output.text,
            tool_calls_count,
            tool_failures_count: completed_tools.iter().filter(|tool| tool.status != "success").count(),
            requests: output.model_turns,
            usage,
            status,
            metrics,
        })
    }
}
