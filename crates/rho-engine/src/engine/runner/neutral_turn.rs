use super::sink::{TerminalApprovalSink, TerminalSinkConfig};
use super::turn::{RunStatus, TurnOutput, TurnRequest};
use crate::engine::context::ProjectContext;
use crate::engine::metrics::{StructuralUsage, TerminalStatus};
use crate::engine::provider::host_loop::{
    CancellationSignal, NeutralTurnObserver, NeutralTurnRequest, NeutralTurnRuntime, NeutralTurnTerminal,
    ProviderUsage, run_neutral_turn,
};
use crate::engine::{AgentBackend, AgentEngine};
use rho_core::approval::{ApprovalCapability, approval_context};
use rho_core::error::{AppError, Result};
use rho_core::presentation::presenter::Presenter;
use rho_core::session::SessionEventKind;
use rho_core::session::context::context_memory;
use rho_sdk::contract::{MessageContent, MessageRole, ModelMessage};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct TurnObserver {
    sink: Arc<TerminalApprovalSink>,
    tool_calls: AtomicUsize,
}

impl NeutralTurnObserver for TurnObserver {
    fn text_delta(&self, text: &str) {
        self.sink.emit_text(text);
    }

    fn tool_call(&self, _call: &crate::engine::provider::host_loop::NeutralToolCall) {
        self.tool_calls.fetch_add(1, Ordering::Relaxed);
        self.sink.resume_model_spinner();
    }
}

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
        let context = ProjectContext::discover(std::env::current_dir()?, Some(&self.config.config_dir)).await;
        self.session_manager
            .append_event(
                SessionEventKind::UserMessage,
                serde_json::json!({"prompt": request.prompt}),
            )
            .await?;
        let ext_context = self.extension_context();
        let mut preamble = context.build_system_prompt();
        let mut turn_event = rho_host::TurnEvent {
            prompt: request.prompt,
            system_prompt: &mut preamble,
        };
        self.extension_registry
            .dispatch_before_turn(&mut turn_event, &ext_context)
            .await?;

        self.run_tracker.start();
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
        });
        tools.begin_turn(&mut tool_context).await;
        let observer = TurnObserver {
            sink: sink.clone(),
            tool_calls: AtomicUsize::new(0),
        };
        let prior_history = context_memory(
            self.session_manager.clone(),
            self.config.context_window_messages,
            self.config.compaction_max_bytes,
        )
        .load(&self.session_manager.session_id)
        .await
        .map_err(|_| AppError::Session("Model-visible session history could not be loaded".to_string()))?;
        let mut messages = vec![ModelMessage {
            role: MessageRole::System,
            content: vec![MessageContent::Text { text: preamble }],
        }];
        messages.extend(rig_history_to_neutral(&prior_history));
        let new_history_start = messages.len();
        messages.push(ModelMessage {
            role: MessageRole::User,
            content: vec![MessageContent::Text {
                text: request.prompt.to_string(),
            }],
        });
        let mut checkpoint = None;
        let mut max_turns = self.config.max_turns;

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
                    messages,
                    credential: credential.clone(),
                    max_output_tokens: self.config.max_output_tokens,
                    tools: tools.provider_definitions(),
                    max_turns,
                    checkpoint,
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
        let usage = structural_usage(output.usage);
        let usage = usage.has_values().then_some(usage);
        if let Some(usage) = usage {
            self.record_usage(usage);
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

struct ExternalTurnArtifacts {
    output: crate::engine::provider::host_loop::NeutralTurnOutput,
    tool_calls_count: usize,
    completed_tools: Vec<super::sink::CompletedTool>,
}

fn rig_history_to_neutral(history: &[rig::message::Message]) -> Vec<ModelMessage> {
    let mut messages = Vec::new();
    for message in history {
        match message {
            rig::message::Message::System { .. } => {}
            rig::message::Message::User { content } => {
                for item in content {
                    match item {
                        rig::message::UserContent::Text(text) => messages.push(ModelMessage {
                            role: MessageRole::User,
                            content: vec![MessageContent::Text {
                                text: text.text.clone(),
                            }],
                        }),
                        rig::message::UserContent::ToolResult(result) => {
                            let content = result
                                .content
                                .iter()
                                .filter_map(|item| item.as_text().map(str::to_string))
                                .collect::<Vec<_>>()
                                .join("\n");
                            messages.push(ModelMessage {
                                role: MessageRole::Tool,
                                content: vec![MessageContent::ToolResult {
                                    call_id: result.call.as_str().to_string(),
                                    content,
                                    is_error: false,
                                }],
                            });
                        }
                        _ => {}
                    }
                }
            }
            rig::message::Message::Assistant { content, .. } => {
                for item in content {
                    match item {
                        rig::message::AssistantContent::Text(text) => messages.push(ModelMessage {
                            role: MessageRole::Assistant,
                            content: vec![MessageContent::Text {
                                text: text.text.clone(),
                            }],
                        }),
                        rig::message::AssistantContent::ToolCall(call) => messages.push(ModelMessage {
                            role: MessageRole::Assistant,
                            content: vec![MessageContent::ToolCall {
                                call_id: call.id.as_str().to_string(),
                                tool_id: format!("tool:{}", call.function.name)
                                    .parse()
                                    .unwrap_or_else(|_| "tool:unknown".parse().unwrap()),
                                arguments: call.function.arguments.clone(),
                            }],
                        }),
                        _ => {}
                    }
                }
            }
        }
    }
    messages
}

fn neutral_history_to_rig(messages: &[ModelMessage]) -> Vec<rig::message::Message> {
    let tool_names: std::collections::HashMap<&str, &str> = messages
        .iter()
        .filter_map(|message| match message {
            ModelMessage {
                role: MessageRole::Assistant,
                content,
            } => content.iter().find_map(|item| match item {
                MessageContent::ToolCall { call_id, tool_id, .. } => Some((call_id.as_str(), tool_id.name())),
                _ => None,
            }),
            _ => None,
        })
        .collect();
    let mut canonical = Vec::new();
    let mut index = 0;
    while index < messages.len() {
        match &messages[index] {
            ModelMessage {
                role: MessageRole::System,
                ..
            } => index += 1,
            ModelMessage {
                role: MessageRole::User,
                content,
            } => {
                for item in content {
                    if let MessageContent::Text { text } = item {
                        canonical.push(rig::message::Message::user(text.clone()));
                    }
                }
                index += 1;
            }
            ModelMessage {
                role: MessageRole::Assistant,
                content,
            } => {
                let mut assistant_content = Vec::new();
                for item in content {
                    match item {
                        MessageContent::Text { text } => {
                            assistant_content.push(rig::message::AssistantContent::text(text.clone()));
                        }
                        MessageContent::ToolCall {
                            call_id,
                            tool_id,
                            arguments,
                        } => assistant_content.push(rig::message::AssistantContent::ToolCall(
                            rig::message::ToolCall::new(
                                rig::message::ToolCallId::new_or_mint(call_id.clone()),
                                rig::message::ToolFunction::new(tool_id.name().to_string(), arguments.clone()),
                            ),
                        )),
                        MessageContent::ToolResult { .. } => {}
                    }
                }
                if !assistant_content.is_empty() {
                    canonical.push(rig::message::Message::Assistant {
                        id: None,
                        content: assistant_content,
                    });
                }
                index += 1;
            }
            ModelMessage {
                role: MessageRole::Tool,
                ..
            } => {
                let mut results = Vec::new();
                while let Some(ModelMessage {
                    role: MessageRole::Tool,
                    content,
                }) = messages.get(index)
                {
                    for item in content {
                        if let MessageContent::ToolResult { call_id, content, .. } = item {
                            results.push(rig::message::UserContent::ToolResult(rig::message::ToolResult {
                                call: rig::message::ToolCallId::new_or_mint(call_id.clone()),
                                provider: None,
                                name: tool_names.get(call_id.as_str()).unwrap_or(&"unknown").to_string(),
                                content: vec![rig::message::ToolResultContent::Text(rig::message::Text::new(
                                    content.clone(),
                                ))],
                            }));
                        }
                    }
                    index += 1;
                }
                if !results.is_empty() {
                    canonical.push(rig::message::Message::User { content: results });
                }
            }
        }
    }
    canonical
}

fn structural_usage(usage: ProviderUsage) -> StructuralUsage {
    StructuralUsage {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        total_tokens: usage.input_tokens.saturating_add(usage.output_tokens),
        cached_input_tokens: None,
        cache_creation_input_tokens: None,
        tool_use_prompt_tokens: None,
        reasoning_tokens: None,
    }
}
