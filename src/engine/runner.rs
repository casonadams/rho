use crate::engine::AgentEngine;
use crate::engine::context::ProjectContext;
use crate::engine::metrics::{RunMetrics, StructuralUsage, TerminalStatus};
use crate::engine::runtime::build_runner;
use crate::error::{AppError, Result};
use crate::intent::model::IntentSpec;
use crate::session::context::context_memory;
use crate::session::{SessionEventKind, SessionManager};
use crate::tools::policy::ExecutionClass;
use crate::tools::{
    ApprovalCapability, ApprovalDecision, ApprovalEventSink, ApprovalHook, ApprovalRequest, RepeatedCallHook,
    ToolEvent, approval_context,
};
use crate::ui::TerminalRenderer;
use crate::ui::render::{ApprovalResult, BashApproval, ToolLine, summarize_tool_output};
use async_trait::async_trait;
use futures::StreamExt;
use indicatif::ProgressBar;
use rig::agent::{MultiTurnStreamItem, PromptResponse};
#[cfg(test)]
use rig::completion::Usage;
use rig::completion::{FinishReason, PromptError};
use rig::memory::ConversationMemory;
use rig::streaming::{StreamedAssistantContent, StreamedUserContent};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunStatus {
    Completed,
    ContentFiltered,
}

pub type UsageDetails = StructuralUsage;

#[derive(Debug)]
pub struct TurnRequest<'a> {
    pub prompt: &'a str,
    pub intent: Option<&'a IntentSpec>,
}

#[derive(Debug)]
pub struct TurnOutput {
    pub final_text: String,
    pub tool_calls_count: usize,
    pub tool_failures_count: usize,
    pub requests: usize,
    pub usage: Option<UsageDetails>,
    pub status: RunStatus,
    pub metrics: RunMetrics,
}

#[derive(Clone)]
struct CompletedTool {
    internal_call_id: String,
    name: String,
    arguments: Value,
    output: String,
    status: String,
}

struct TurnArtifacts {
    response: PromptResponse,
    tool_calls_count: usize,
    completed_tools: Vec<CompletedTool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum DisplayKind {
    #[default]
    None,
    Thinking,
    Text,
    Tool,
}

struct TerminalSinkState {
    auto_approve: bool,
    spinner: Option<ProgressBar>,
    pending: HashMap<String, (String, Value)>,
    reasoning: Vec<String>,
    completed: Vec<CompletedTool>,
    last_display: DisplayKind,
}

struct TerminalApprovalSink {
    renderer: TerminalRenderer,
    session_manager: SessionManager,
    run_tracker: crate::engine::metrics::RunTracker,
    state: Mutex<TerminalSinkState>,
}

struct TerminalSinkConfig {
    model_spinner: ProgressBar,
    auto_approve: bool,
    run_tracker: crate::engine::metrics::RunTracker,
}

impl TerminalApprovalSink {
    fn new(renderer: &TerminalRenderer, config: TerminalSinkConfig, session_manager: SessionManager) -> Arc<Self> {
        Arc::new(Self {
            renderer: renderer.clone(),
            session_manager,
            run_tracker: config.run_tracker,
            state: Mutex::new(TerminalSinkState {
                auto_approve: config.auto_approve,
                spinner: Some(config.model_spinner),
                pending: HashMap::new(),
                reasoning: Vec::new(),
                completed: Vec::new(),
                last_display: DisplayKind::None,
            }),
        })
    }

    fn finish_spinner(&self) {
        if let Ok(mut state) = self.state.lock() {
            clear_spinner(&mut state);
        }
    }

    fn completed(&self) -> Vec<CompletedTool> {
        self.state
            .lock()
            .map(|state| state.completed.clone())
            .unwrap_or_default()
    }

    /// Render buffered reasoning as a distinct, boxed "Thinking" block so it never
    /// runs into the streamed output text. Any pending markdown is flushed first to
    /// preserve ordering.
    fn flush_reasoning(&self) {
        let (reasoning, prev) = self
            .state
            .lock()
            .map(|mut state| {
                if state.reasoning.is_empty() {
                    return (Vec::new(), state.last_display);
                }
                let prev = state.last_display;
                state.last_display = DisplayKind::Thinking;
                (std::mem::take(&mut state.reasoning), prev)
            })
            .unwrap_or_default();
        if reasoning.is_empty() {
            return;
        }
        if prev == DisplayKind::Tool {
            println!();
        }
        self.renderer.flush();
        self.renderer.print_thinking(&reasoning.join(""));
    }

    /// Stream a reasoning token: clear any spinner so thinking is readable, then buffer
    /// it until the model transitions to real output (or the turn ends).
    fn emit_reasoning(&self, text: &str) {
        self.finish_spinner();
        if let Ok(mut state) = self.state.lock() {
            let redacted = self.session_manager.redact_credentials(text);
            if let Some(last) = state.reasoning.last()
                && !last.is_empty()
                && (last.ends_with('.') || last.ends_with('!') || last.ends_with('?'))
                && !redacted.starts_with(' ')
                && !redacted.starts_with('\n')
            {
                state.reasoning.push(format!(" {redacted}"));
                return;
            }
            state.reasoning.push(redacted);
        }
    }

    /// Stream an output token. Reasoning buffered so far is rendered as a block first
    /// so thinking and output never interleave on the same line.
    fn emit_text(&self, text: &str) {
        self.flush_reasoning();
        self.finish_spinner();
        if let Ok(mut state) = self.state.lock() {
            if state.last_display == DisplayKind::Tool || state.last_display == DisplayKind::Thinking {
                println!();
            }
            state.last_display = DisplayKind::Text;
        }
        self.renderer
            .print_token(&self.session_manager.redact_credentials(text));
    }

    fn flush_display(&self) {
        self.flush_reasoning();
        self.renderer.flush();
        if let Ok(state) = self.state.lock()
            && state.last_display == DisplayKind::Tool
        {
            println!();
        }
    }
}

fn clear_spinner(state: &mut TerminalSinkState) {
    if let Some(spinner) = state.spinner.take() {
        spinner.finish_and_clear();
    }
}

fn needs_approval(state: &TerminalSinkState, class: &ExecutionClass) -> bool {
    !state.auto_approve && !class.allows_without_approval()
}

#[async_trait]
impl ApprovalEventSink for TerminalApprovalSink {
    async fn request_approval(&self, request: ApprovalRequest) -> ApprovalDecision {
        self.finish_spinner();
        self.flush_reasoning();
        self.renderer.flush();
        let arguments = redact_value(&self.session_manager, &request.arguments);

        let result = if request.tool_name == "bash" {
            let command = arguments.get("command").and_then(Value::as_str).unwrap_or_default();
            self.renderer.prompt_bash_approval(BashApproval {
                command,
                tier: request.tier,
                reasons: &request.reasons,
            })
        } else {
            self.renderer.prompt_tool_approval(&request.tool_name, &arguments)
        };
        match result {
            ApprovalResult::Approved => ApprovalDecision::Approved,
            ApprovalResult::Denied { reason } => ApprovalDecision::Denied { reason },
        }
    }

    fn emit(&self, event: ToolEvent) {
        match event {
            ToolEvent::CallClassified {
                internal_call_id,
                tool_name,
                arguments,
                class,
            } => {
                self.run_tracker.tool_called();
                self.flush_reasoning();
                let arguments = redact_value(&self.session_manager, &arguments);
                let mut state = self.state.lock().unwrap_or_else(|_| unreachable!());
                clear_spinner(&mut state);
                if !needs_approval(&state, &class) && tool_name != "ask_user" && tool_name != "ask_user_question" {
                    state.spinner = Some(self.renderer.start_tool_spinner(&tool_name, &arguments));
                }
                state.pending.insert(internal_call_id, (tool_name, arguments));
            }
            ToolEvent::ApprovalGranted { internal_call_id, .. } => {
                if let Ok(mut state) = self.state.lock()
                    && let Some((tool_name, arguments)) = state.pending.get(&internal_call_id)
                    && tool_name != "ask_user"
                    && tool_name != "ask_user_question"
                {
                    state.spinner = Some(self.renderer.start_tool_spinner(tool_name, arguments));
                }
            }
            ToolEvent::ApprovalDenied { .. } => {}
            ToolEvent::Finished {
                internal_call_id,
                tool_name,
                arguments,
                output,
                status,
            } => {
                self.run_tracker.tool_finished(&status);
                if let Ok(mut state) = self.state.lock() {
                    clear_spinner(&mut state);
                    state.pending.remove(&internal_call_id);
                    state.last_display = DisplayKind::Tool;
                    let arguments = redact_value(&self.session_manager, &arguments);
                    let output = self.session_manager.redact_credentials(&output);
                    let output_summary = summarize_tool_output(&output);
                    self.renderer.finish_tool_line(ToolLine {
                        name: &tool_name,
                        arguments: &arguments,
                        is_error: status != "success",
                        output_summary: &output_summary,
                    });
                    state.completed.push(CompletedTool {
                        internal_call_id,
                        name: tool_name,
                        arguments,
                        output,
                        status,
                    });
                }
            }
        }
    }
}

impl AgentEngine {
    pub async fn run_turn(&self, request: TurnRequest<'_>, renderer: &TerminalRenderer) -> Result<TurnOutput> {
        let context = ProjectContext::discover(std::env::current_dir()?).await;
        let preamble = context.build_system_prompt(request.intent);
        self.session_manager
            .append_event(
                SessionEventKind::UserMessage,
                serde_json::json!({ "prompt": request.prompt }),
            )
            .await?;
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
        let spinner = renderer.start_spinner(&format!(
            "{}:{} thinking",
            self.config.model,
            self.context_usage_display()
        ));
        let sink = TerminalApprovalSink::new(
            renderer,
            TerminalSinkConfig {
                model_spinner: spinner,
                auto_approve: self.config.auto_approve,
                run_tracker: self.run_tracker.clone(),
            },
            self.session_manager.clone(),
        );
        let capability = ApprovalCapability::new(self.config.auto_approve, sink.clone());
        let mut current_prompt = request.prompt.to_string();
        let mut total_tool_calls = 0;
        let mut current_budget = self.config.max_turns;

        loop {
            let runner = build_runner(&self.agent, &current_prompt)
                .conversation(self.session_manager.session_id.clone())
                .preamble(&preamble)
                .max_turns(current_budget)
                .tool_context(approval_context(capability.clone()))
                .add_hook(RepeatedCallHook::new(std::env::current_dir()?).with_sink(sink.clone()))
                .add_hook(ApprovalHook::new(capability.clone()));
            let runner = match checkpoint.as_ref() {
                Some(pending) => runner.history(continuation_history(&visible_history, pending)),
                None => runner,
            };
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
                            if !self.config.auto_approve && renderer.prompt_continue_budget(max_turns) {
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
                    MultiTurnStreamItem::StreamAssistantItem(item) => {
                        for event in display_events(item, &mut reasoning_parts) {
                            match event {
                                DisplayEvent::Text(text) => sink.emit_text(&text),
                                DisplayEvent::Reasoning(text) => sink.emit_reasoning(&text),
                                DisplayEvent::ToolCall => {
                                    sink.flush_reasoning();
                                    total_tool_calls += 1;
                                }
                            }
                        }
                    }
                    MultiTurnStreamItem::FinalResponse(response) => final_response = Some(response),
                    MultiTurnStreamItem::CompletionCall(call) => self.run_tracker.completion(call),
                    MultiTurnStreamItem::StreamUserItem(StreamedUserContent::ToolResult { .. })
                    | MultiTurnStreamItem::ToolExecutionCommitted { .. }
                    | MultiTurnStreamItem::ModelTurnRetried { .. } => {}
                }
            }

            if budget_hit {
                current_prompt = "Please continue where you left off and finish the task.".to_string();
                current_budget = 50;
                continue;
            }

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
            return self
                .finish_turn(TurnArtifacts {
                    response,
                    tool_calls_count: total_tool_calls,
                    completed_tools: sink.completed(),
                })
                .await;
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

    async fn record_failed_metrics(&self, error: &AppError) -> Result<()> {
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

    async fn record_run_summary(&self, metrics: &RunMetrics) -> Result<()> {
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
        self.record_usage(usage);
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

#[derive(Debug, PartialEq, Eq)]
enum DisplayEvent {
    Text(String),
    Reasoning(String),
    ToolCall,
}

fn display_events(item: StreamedAssistantContent, reasoning_parts: &mut HashSet<String>) -> Vec<DisplayEvent> {
    match item {
        StreamedAssistantContent::Text(text) => vec![DisplayEvent::Text(text.text)],
        StreamedAssistantContent::ReasoningDelta { id, reasoning, .. } => {
            reasoning_parts.insert(id);
            vec![DisplayEvent::Reasoning(reasoning)]
        }
        StreamedAssistantContent::Reasoning { reasoning, id } if !reasoning_parts.contains(&id) => reasoning
            .content
            .into_iter()
            .filter_map(|content| match content {
                rig::message::ReasoningContent::Text { text, .. } | rig::message::ReasoningContent::Summary(text) => {
                    Some(DisplayEvent::Reasoning(text))
                }
                rig::message::ReasoningContent::Encrypted(_) | rig::message::ReasoningContent::Redacted { .. } => None,
            })
            .collect(),
        StreamedAssistantContent::ToolCall { .. } => vec![DisplayEvent::ToolCall],
        StreamedAssistantContent::ToolCallDelta { .. }
        | StreamedAssistantContent::Reasoning { .. }
        | StreamedAssistantContent::Final(_)
        | StreamedAssistantContent::Unknown(_) => Vec::new(),
    }
}

fn budget_history(error: &rig::agent::StreamingError) -> Option<(usize, Vec<rig::message::Message>)> {
    let rig::agent::StreamingError::Prompt(error) = error else {
        return None;
    };
    let PromptError::MaxTurnsError {
        max_turns,
        chat_history,
        ..
    } = error.as_ref()
    else {
        return None;
    };
    Some((*max_turns, chat_history.as_ref().clone()))
}

fn checkpoint_messages(
    visible_history: &[rig::message::Message],
    full_history: &[rig::message::Message],
) -> Result<Vec<rig::message::Message>> {
    full_history
        .strip_prefix(visible_history)
        .filter(|messages| !messages.is_empty())
        .map(<[rig::message::Message]>::to_vec)
        .ok_or_else(|| AppError::Session("Budget checkpoint did not match the model-visible history".to_string()))
}

fn continuation_history(
    visible_history: &[rig::message::Message],
    checkpoint: &[rig::message::Message],
) -> Vec<rig::message::Message> {
    let mut history = Vec::with_capacity(visible_history.len() + checkpoint.len());
    history.extend_from_slice(visible_history);
    history.extend_from_slice(checkpoint);
    history
}

fn map_streaming_error(error: rig::agent::StreamingError) -> AppError {
    match error {
        rig::agent::StreamingError::Completion(error) => map_completion_error(error),
        rig::agent::StreamingError::Prompt(error) => map_prompt_error(*error),
    }
}

fn map_prompt_error(error: PromptError) -> AppError {
    match error {
        PromptError::MaxTurnsError { max_turns, .. } => AppError::ModelBudgetExhausted { max_turns },
        PromptError::PromptCancelled { reason, .. } => AppError::Cancelled(redact_text(&reason)),
        PromptError::UnknownToolCall { tool_name, .. } => AppError::InvalidToolCall(tool_name),
        PromptError::CompletionError(error) => map_completion_error(error),
        PromptError::MemoryError(_) => AppError::Provider("Conversation memory failed".to_string()),
    }
}

fn map_completion_error(error: rig::completion::CompletionError) -> AppError {
    if matches!(
        &error,
        rig::completion::CompletionError::ResponseError(message) if message.contains("ContentFilter")
    ) {
        return AppError::ContentFiltered;
    }
    let status = error.provider_response_status();
    match status {
        Some(status) => AppError::Provider(format!("Model provider request failed with HTTP {status}")),
        None => AppError::Provider("Model provider request failed".to_string()),
    }
}

fn redact_value(session: &SessionManager, value: &Value) -> Value {
    match value {
        Value::String(value) => Value::String(session.redact_credentials(value)),
        Value::Array(values) => Value::Array(values.iter().map(|value| redact_value(session, value)).collect()),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), redact_value(session, value)))
                .collect(),
        ),
        value => value.clone(),
    }
}

fn redact_text(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    if ["api_key", "access_token", "refresh_token", "authorization", "bearer "]
        .iter()
        .any(|marker| lower.contains(marker))
    {
        "sensitive upstream detail redacted".to_string()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthStore;
    use crate::config::Config;
    use crate::engine::runtime::{CodingRuntime, build_coding_agent};
    use crate::session::SessionManager;
    use crate::tools::bash_ast::RiskTier;
    use rig::agent::ModelHandle;
    use rig::message::{AssistantContent, Message, UserContent};
    use rig::test_utils::{MockCompletionModel, MockStreamEvent};

    fn test_engine(model: MockCompletionModel, config: Config) -> AgentEngine {
        let dir = std::env::temp_dir().join(format!("runner_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let session_manager = SessionManager::new(&dir, None).unwrap();
        test_engine_with_session(model, config, session_manager)
    }

    fn test_engine_with_session(
        model: MockCompletionModel,
        config: Config,
        session_manager: SessionManager,
    ) -> AgentEngine {
        let base_dir = session_manager.file_path.parent().unwrap();
        let agent = build_coding_agent(
            ModelHandle::new(model),
            &config,
            CodingRuntime {
                base_dir,
                memory: session_manager.clone(),
            },
        )
        .unwrap();
        AgentEngine {
            config,
            session_manager,
            agent,
            last_usage: Mutex::new(None),
            run_tracker: crate::engine::metrics::RunTracker::default(),
        }
    }

    fn terminal_session() -> SessionManager {
        let dir = std::env::temp_dir().join(format!("sink_test_{}", uuid::Uuid::new_v4()));
        SessionManager::new(&dir, None).unwrap()
    }

    fn final_event(usage: Usage) -> MockStreamEvent {
        MockStreamEvent::final_response(usage)
    }

    fn request(prompt: &str) -> TurnRequest<'_> {
        TurnRequest { prompt, intent: None }
    }

    #[test]
    fn renderer_events_preserve_reasoning_text_order_without_duplicates() {
        let mut reasoning_parts = HashSet::new();
        let events = [
            StreamedAssistantContent::ReasoningDelta {
                id: "reasoning-1".to_string(),
                provider_id: None,
                reasoning: "think".to_string(),
            },
            StreamedAssistantContent::Reasoning {
                id: "reasoning-1".to_string(),
                reasoning: rig::message::Reasoning::new("think"),
            },
            StreamedAssistantContent::text("answer"),
        ]
        .into_iter()
        .flat_map(|item| display_events(item, &mut reasoning_parts))
        .collect::<Vec<_>>();

        assert_eq!(
            events,
            [
                DisplayEvent::Reasoning("think".to_string()),
                DisplayEvent::Text("answer".to_string())
            ]
        );
    }

    #[test]
    fn reasoning_buffers_and_flushes_on_output() {
        let renderer = TerminalRenderer::default();
        let sink = TerminalApprovalSink::new(
            &renderer,
            TerminalSinkConfig {
                model_spinner: renderer.start_spinner("model"),
                auto_approve: true,
                run_tracker: crate::engine::metrics::RunTracker::default(),
            },
            terminal_session(),
        );
        sink.emit_reasoning("think ");
        sink.emit_reasoning("harder");
        assert_eq!(sink.state.lock().unwrap().reasoning.join(""), "think harder");

        // Transitioning to real output flushes the buffered thinking block.
        sink.emit_text("answer");
        assert!(sink.state.lock().unwrap().reasoning.is_empty());
    }

    #[test]
    fn approval_required_holds_spinner_until_granted() {
        let renderer = TerminalRenderer::default();
        let sink = TerminalApprovalSink::new(
            &renderer,
            TerminalSinkConfig {
                model_spinner: renderer.start_spinner("model"),
                auto_approve: false,
                run_tracker: crate::engine::metrics::RunTracker::default(),
            },
            terminal_session(),
        );
        sink.emit(ToolEvent::CallClassified {
            internal_call_id: "call-1".to_string(),
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({ "command": "rm -rf target" }),
            class: ExecutionClass::ApprovalRequired {
                tier: RiskTier::HighRisk,
                reasons: vec!["destructive".to_string()],
            },
        });
        // No spinner while the approval prompt is on screen.
        assert!(sink.state.lock().unwrap().spinner.is_none());
        assert!(sink.state.lock().unwrap().pending.contains_key("call-1"));

        sink.emit(ToolEvent::ApprovalGranted {
            internal_call_id: "call-1".to_string(),
            tool_name: "bash".to_string(),
        });
        // The spinner starts only once execution is actually approved.
        assert!(sink.state.lock().unwrap().spinner.is_some());
    }

    #[test]
    fn approval_denied_leaves_no_spinner() {
        let renderer = TerminalRenderer::default();
        let sink = TerminalApprovalSink::new(
            &renderer,
            TerminalSinkConfig {
                model_spinner: renderer.start_spinner("model"),
                auto_approve: false,
                run_tracker: crate::engine::metrics::RunTracker::default(),
            },
            terminal_session(),
        );
        sink.emit(ToolEvent::CallClassified {
            internal_call_id: "call-1".to_string(),
            tool_name: "write".to_string(),
            arguments: serde_json::json!({ "path": "/tmp/x", "content": "y" }),
            class: ExecutionClass::ApprovalRequired {
                tier: RiskTier::Mutating,
                reasons: vec!["outside".to_string()],
            },
        });
        sink.emit(ToolEvent::ApprovalDenied {
            internal_call_id: "call-1".to_string(),
            tool_name: "write".to_string(),
        });
        assert!(sink.state.lock().unwrap().spinner.is_none());
    }

    #[test]
    fn reasoning_flushes_before_tool_classification() {
        let renderer = TerminalRenderer::default();
        let sink = TerminalApprovalSink::new(
            &renderer,
            TerminalSinkConfig {
                model_spinner: renderer.start_spinner("model"),
                auto_approve: false,
                run_tracker: crate::engine::metrics::RunTracker::default(),
            },
            terminal_session(),
        );
        sink.emit_reasoning("pondering next step");
        assert_eq!(sink.state.lock().unwrap().reasoning.join(""), "pondering next step");

        sink.emit(ToolEvent::CallClassified {
            internal_call_id: "call-1".to_string(),
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({ "command": "cargo test" }),
            class: ExecutionClass::ReadOnly,
        });

        assert!(sink.state.lock().unwrap().reasoning.is_empty());
    }

    #[tokio::test]
    async fn final_text_streams_once_and_usage_can_be_unavailable() {
        let model =
            MockCompletionModel::from_stream_turns([[MockStreamEvent::text("final text"), final_event(Usage::new())]]);
        let engine = test_engine(model, Config::default());
        let output = engine
            .run_turn(request("prompt"), &TerminalRenderer::default())
            .await
            .unwrap();
        assert_eq!(output.final_text, "final text");
        assert_eq!(output.usage, None);
        assert_eq!(output.requests, 1);
        assert!(!output.metrics.usage_available);
        assert_eq!(output.metrics.model_turns, 1);
    }

    #[tokio::test]
    async fn two_prompts_receive_prior_canonical_history_exactly_once() {
        let model = MockCompletionModel::from_stream_turns([
            [MockStreamEvent::text("first answer"), final_event(Usage::new())],
            [MockStreamEvent::text("second answer"), final_event(Usage::new())],
        ]);
        let engine = test_engine(model.clone(), Config::default());
        engine
            .run_turn(request("first prompt"), &TerminalRenderer::default())
            .await
            .unwrap();
        engine
            .run_turn(request("second prompt"), &TerminalRenderer::default())
            .await
            .unwrap();

        let second = &model.requests()[1].chat_history;
        let encoded = serde_json::to_string(second).unwrap();
        assert_eq!(second.len(), 4, "{encoded}");
        assert_eq!(encoded.matches("first prompt").count(), 1);
        assert_eq!(encoded.matches("first answer").count(), 1);
        assert_eq!(encoded.matches("second prompt").count(), 1);
    }

    #[tokio::test]
    async fn process_style_reopen_resumes_canonical_history_once() {
        let first_model = MockCompletionModel::from_stream_turns([[
            MockStreamEvent::text("persisted answer"),
            final_event(Usage::new()),
        ]]);
        let first = test_engine(first_model, Config::default());
        first
            .run_turn(request("persisted prompt"), &TerminalRenderer::default())
            .await
            .unwrap();
        let id = first.session_manager.session_id.clone();
        let dir = first.session_manager.file_path.parent().unwrap().to_path_buf();
        drop(first);

        let resumed_store = SessionManager::new(&dir, Some(&id)).unwrap();
        let resumed_model = MockCompletionModel::from_stream_turns([[
            MockStreamEvent::text("resumed answer"),
            final_event(Usage::new()),
        ]]);
        let resumed = test_engine_with_session(resumed_model.clone(), Config::default(), resumed_store);
        resumed
            .run_turn(request("resume prompt"), &TerminalRenderer::default())
            .await
            .unwrap();

        let history = &resumed_model.requests()[0].chat_history;
        let encoded = serde_json::to_string(history).unwrap();
        assert_eq!(history.len(), 4, "{encoded}");
        assert_eq!(encoded.matches("persisted prompt").count(), 1);
        assert_eq!(encoded.matches("persisted answer").count(), 1);
    }

    #[tokio::test]
    async fn model_rebuild_preserves_compatible_history_without_duplication() {
        let config = Config {
            provider: "ollama".to_string(),
            model: "first-local-model".to_string(),
            ..Config::default()
        };
        let model = MockCompletionModel::from_stream_turns([[
            MockStreamEvent::text("stored answer"),
            final_event(Usage::new()),
        ]]);
        let engine = test_engine(model, config.clone());
        engine
            .run_turn(request("stored prompt"), &TerminalRenderer::default())
            .await
            .unwrap();
        let id = engine.session_manager.session_id.clone();
        let rebuilt = engine
            .rebuild(
                Config {
                    model: "second-local-model".to_string(),
                    ..config
                },
                AuthStore::default(),
            )
            .await
            .unwrap();

        assert_eq!(rebuilt.session_manager.session_id, id);
        let encoded = serde_json::to_string(&rebuilt.session_manager.load_messages().await.unwrap()).unwrap();
        assert_eq!(encoded.matches("stored prompt").count(), 1);
        assert_eq!(encoded.matches("stored answer").count(), 1);
    }

    #[tokio::test]
    async fn one_tool_round_preserves_canonical_call_and_one_result() {
        let model = MockCompletionModel::from_stream_turns([
            [
                MockStreamEvent::tool_call("call-1", "read", serde_json::json!({"path": "missing"})),
                final_event(Usage::new()),
            ],
            [MockStreamEvent::text("done"), final_event(Usage::new())],
        ]);
        let engine = test_engine(model.clone(), Config::default());
        let output = engine
            .run_turn(request("read"), &TerminalRenderer::default())
            .await
            .unwrap();

        assert_eq!(output.tool_calls_count, 1);
        let request = &model.requests()[1];
        let assistant_calls = request
            .chat_history
            .iter()
            .filter_map(|message| match message {
                Message::Assistant { content, .. } => Some(
                    content
                        .iter()
                        .filter(|content| matches!(content, AssistantContent::ToolCall(_)))
                        .count(),
                ),
                _ => None,
            })
            .sum::<usize>();
        let results = request
            .chat_history
            .iter()
            .filter_map(|message| match message {
                Message::User { content } => Some(
                    content
                        .iter()
                        .filter(|content| matches!(content, UserContent::ToolResult(_)))
                        .count(),
                ),
                _ => None,
            })
            .sum::<usize>();
        assert_eq!((assistant_calls, results), (1, 1));
    }

    #[tokio::test]
    async fn multiple_tool_calls_have_one_correlated_result_each() {
        let model = MockCompletionModel::from_stream_turns([
            vec![
                MockStreamEvent::tool_call("call-1", "read", serde_json::json!({"path": "missing-a"})),
                MockStreamEvent::tool_call("call-2", "read", serde_json::json!({"path": "missing-b"})),
                final_event(Usage::new()),
            ],
            vec![MockStreamEvent::text("done"), final_event(Usage::new())],
        ]);
        let engine = test_engine(model.clone(), Config::default());
        let output = engine
            .run_turn(request("read both"), &TerminalRenderer::default())
            .await
            .unwrap();

        assert_eq!(output.tool_calls_count, 2);
        assert_eq!(output.tool_failures_count, 2);
        let request = &model.requests()[1];
        let serialized = serde_json::to_value(&request.chat_history).unwrap();
        let calls = serialized.to_string().matches("toolcall").count();
        let results = serialized.to_string().matches("toolresult").count();
        assert_eq!((calls, results), (2, 2));
    }

    #[tokio::test]
    async fn malformed_tool_arguments_are_model_visible_tool_failures() {
        let model = MockCompletionModel::from_stream_turns([
            [
                MockStreamEvent::tool_call("call-1", "read", serde_json::json!({"unexpected": true})),
                final_event(Usage::new()),
            ],
            [MockStreamEvent::text("recovered"), final_event(Usage::new())],
        ]);
        let engine = test_engine(
            model.clone(),
            Config {
                auto_approve: true,
                ..Config::default()
            },
        );
        let output = engine
            .run_turn(request("read"), &TerminalRenderer::default())
            .await
            .unwrap();

        assert_eq!(output.tool_failures_count, 1);
        assert!(format!("{:?}", model.requests()[1]).contains("failed to parse tool arguments"));
    }

    #[tokio::test]
    async fn unknown_tool_calls_fail_without_fallback() {
        let model = MockCompletionModel::from_stream_turns([[
            MockStreamEvent::tool_call("call-1", "unknown", serde_json::json!({})),
            final_event(Usage::new()),
        ]]);
        let engine = test_engine(model, Config::default());
        let error = engine
            .run_turn(request("unknown"), &TerminalRenderer::default())
            .await
            .unwrap_err();

        assert!(matches!(error, AppError::InvalidToolCall(name) if name == "unknown"));
    }

    #[tokio::test]
    async fn normalized_usage_is_exposed_when_available() {
        let usage = Usage {
            input_tokens: 10,
            output_tokens: 4,
            total_tokens: 14,
            cached_input_tokens: 3,
            cache_creation_input_tokens: 2,
            tool_use_prompt_tokens: 1,
            reasoning_tokens: 2,
        };
        let model = MockCompletionModel::from_stream_turns([[MockStreamEvent::text("done"), final_event(usage)]]);
        let engine = test_engine(model, Config::default());
        let output = engine
            .run_turn(request("prompt"), &TerminalRenderer::default())
            .await
            .unwrap();

        assert_eq!(output.usage, Some(usage.into()));
        assert!(output.metrics.usage_available);
        assert_eq!(output.metrics.usage.unwrap().cached_input_tokens, Some(3));
        assert_eq!(output.metrics.usage.unwrap().reasoning_tokens, Some(2));
        assert_eq!(engine.context_usage_display(), "10 input tokens");
    }

    #[tokio::test]
    async fn content_filter_finish_is_distinct() {
        let final_record =
            rig::streaming::StreamFinal::new("mock", Usage::new()).with_finish_reason(FinishReason::ContentFilter);
        let model = MockCompletionModel::from_stream_turns([[
            MockStreamEvent::text("filtered partial"),
            MockStreamEvent::FinalResponse(final_record),
        ]]);
        let engine = test_engine(model, Config::default());
        let output = engine
            .run_turn(request("prompt"), &TerminalRenderer::default())
            .await
            .unwrap();

        assert_eq!(output.status, RunStatus::ContentFiltered);
    }

    #[tokio::test]
    async fn provider_stream_failures_do_not_expose_upstream_details() {
        let model = MockCompletionModel::from_stream_turns([[MockStreamEvent::error(
            "authorization: Bearer credential-sentinel",
        )]]);
        let engine = test_engine(model, Config::default());
        let error = engine
            .run_turn(request("prompt"), &TerminalRenderer::default())
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("Model provider request failed"));
        assert!(!error.contains("credential-sentinel"));
        assert!(!error.contains("Bearer"));
        let persisted = std::fs::read_to_string(&engine.session_manager.file_path).unwrap();
        assert!(!persisted.contains("credential-sentinel"));
        assert!(!persisted.contains("Bearer"));
    }

    #[tokio::test]
    async fn explicit_output_limit_and_max_turn_budget_reach_rig() {
        let config = Config {
            max_output_tokens: Some(321),
            max_turns: 1,
            ..Config::default()
        };
        let model = MockCompletionModel::from_stream_turns([[
            MockStreamEvent::tool_call("call-1", "read", serde_json::json!({"path": "missing"})),
            final_event(Usage::new()),
        ]]);
        let engine = test_engine(model.clone(), config);
        let error = engine
            .run_turn(request("read"), &TerminalRenderer::default())
            .await
            .unwrap_err();

        assert!(matches!(error, AppError::ModelBudgetExhausted { max_turns: 1 }));
        assert_eq!(model.requests()[0].max_tokens, Some(321));
    }

    #[tokio::test]
    async fn budget_exhausted_checkpoint_survives_process_resume_and_promotes_once() {
        let first_model = MockCompletionModel::from_stream_turns([
            [
                MockStreamEvent::tool_call("call-1", "read", serde_json::json!({"path":"missing-a"})),
                final_event(Usage::new()),
            ],
            [
                MockStreamEvent::tool_call("call-2", "read", serde_json::json!({"path":"missing-b"})),
                final_event(Usage::new()),
            ],
        ]);
        let first = test_engine(
            first_model,
            Config {
                auto_approve: true,
                max_turns: 2,
                ..Config::default()
            },
        );
        let error = first
            .run_turn(request("inspect the repository"), &TerminalRenderer::default())
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::ModelBudgetExhausted { max_turns: 2 }));
        assert!(first.session_manager.load_messages().await.unwrap().is_empty());
        let checkpoint = first.session_manager.load_checkpoint().await.unwrap().unwrap();
        assert_eq!(checkpoint.len(), 5);
        let id = first.session_manager.session_id.clone();
        let dir = first.session_manager.file_path.parent().unwrap().to_path_buf();
        drop(first);

        let resumed_store = SessionManager::new(&dir, Some(&id)).unwrap();
        let resumed_model = MockCompletionModel::from_stream_turns([[
            MockStreamEvent::text("repository summary"),
            final_event(Usage::new()),
        ]]);
        let resumed = test_engine_with_session(
            resumed_model.clone(),
            Config {
                auto_approve: true,
                max_turns: 2,
                ..Config::default()
            },
            resumed_store,
        );
        resumed
            .run_turn(request("please continue"), &TerminalRenderer::default())
            .await
            .unwrap();

        let history = &resumed_model.requests()[0].chat_history;
        let encoded = serde_json::to_string(history).unwrap();
        assert_eq!(encoded.matches("inspect the repository").count(), 1);
        assert_eq!(encoded.matches("missing-a").count(), 2);
        assert_eq!(encoded.matches("missing-b").count(), 2);
        assert_eq!(encoded.matches("please continue").count(), 1);
        assert!(resumed.session_manager.load_checkpoint().await.unwrap().is_none());
        assert_eq!(resumed.session_manager.load_messages().await.unwrap().len(), 7);

        drop(resumed);
        let reopened = SessionManager::new(&dir, Some(&id)).unwrap();
        assert!(reopened.load_checkpoint().await.unwrap().is_none());
        assert_eq!(reopened.load_messages().await.unwrap().len(), 7);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn mutating_tools_execute_sequentially() {
        let marker = std::env::temp_dir().join(format!("sequential_marker_{}", uuid::Uuid::new_v4()));
        let model = MockCompletionModel::from_stream_turns([
            vec![
                MockStreamEvent::tool_call(
                    "call-1",
                    "bash",
                    serde_json::json!({"command": format!("sleep 0.05; printf 1 >> {}", marker.display())}),
                ),
                MockStreamEvent::tool_call(
                    "call-2",
                    "bash",
                    serde_json::json!({"command": format!("printf 2 >> {}", marker.display())}),
                ),
                final_event(Usage::new()),
            ],
            vec![MockStreamEvent::text("done"), final_event(Usage::new())],
        ]);
        let engine = test_engine(
            model,
            Config {
                auto_approve: true,
                ..Config::default()
            },
        );
        engine
            .run_turn(request("run"), &TerminalRenderer::default())
            .await
            .unwrap();

        assert_eq!(tokio::fs::read_to_string(&marker).await.unwrap(), "12");
        let _ = tokio::fs::remove_file(marker).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancelled_tool_run_persists_no_incomplete_result() {
        let marker = std::env::temp_dir().join(format!("cancel_marker_{}", uuid::Uuid::new_v4()));
        let model = MockCompletionModel::from_stream_turns([[
            MockStreamEvent::tool_call(
                "call-1",
                "bash",
                serde_json::json!({"command": format!("sleep 2; touch {}", marker.display())}),
            ),
            final_event(Usage::new()),
        ]]);
        let engine = test_engine(
            model,
            Config {
                auto_approve: true,
                ..Config::default()
            },
        );
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            engine.run_turn(request("run"), &TerminalRenderer::default()),
        )
        .await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        assert!(result.is_err());
        engine.record_cancellation("test interrupt").await.unwrap();
        assert!(!marker.exists());
        let events = engine.session_manager.load_events().await.unwrap();
        assert!(!events.iter().any(|event| event.kind == SessionEventKind::ToolResult));
        assert!(events.iter().any(|event| event.kind == SessionEventKind::Cancellation));
        let summary = events
            .iter()
            .find(|event| event.kind == SessionEventKind::RunSummary)
            .unwrap();
        assert_eq!(summary.payload["terminal_status"], "cancelled");
        assert!(engine.session_manager.load_messages().await.unwrap().is_empty());
        let reopened = SessionManager::new(
            engine.session_manager.file_path.parent().unwrap(),
            Some(&engine.session_manager.session_id),
        )
        .unwrap();
        assert!(reopened.load_messages().await.unwrap().is_empty());
    }

    #[test]
    fn provider_error_mapping_redacts_sensitive_bodies() {
        let error = rig::completion::CompletionError::from_http_response(
            reqwest::StatusCode::UNAUTHORIZED,
            "authorization: Bearer credential-sentinel",
        );
        let mapped = map_completion_error(error).to_string();
        assert!(mapped.contains("401"));
        assert!(!mapped.contains("credential-sentinel"));
        assert!(!mapped.contains("Bearer"));
    }

    #[test]
    fn terminal_sink_redacts_secret_tool_arguments_and_results() {
        let dir = std::env::temp_dir().join(format!("sink_secret_{}", uuid::Uuid::new_v4()));
        let session = SessionManager::new_with_secrets(&dir, None, vec!["credential-sentinel".to_string()]).unwrap();
        let renderer = TerminalRenderer::default();
        let sink = TerminalApprovalSink::new(
            &renderer,
            TerminalSinkConfig {
                model_spinner: renderer.start_spinner("model"),
                auto_approve: true,
                run_tracker: crate::engine::metrics::RunTracker::default(),
            },
            session,
        );
        sink.emit(ToolEvent::CallClassified {
            internal_call_id: "call".to_string(),
            tool_name: "read".to_string(),
            arguments: serde_json::json!({"path":"credential-sentinel"}),
            class: ExecutionClass::ReadOnly,
        });
        sink.emit(ToolEvent::Finished {
            internal_call_id: "call".to_string(),
            tool_name: "read".to_string(),
            arguments: serde_json::json!({"path":"credential-sentinel"}),
            output: "credential-sentinel".to_string(),
            status: "error".to_string(),
        });
        let completed = sink.completed();
        assert_eq!(completed.len(), 1);
        assert!(!completed[0].arguments.to_string().contains("credential-sentinel"));
        assert!(!completed[0].output.contains("credential-sentinel"));
        assert!(completed[0].output.contains("[REDACTED]"));
    }

    #[test]
    fn cancellation_reason_is_redacted() {
        assert_eq!(
            redact_text("access_token=credential-sentinel"),
            "sensitive upstream detail redacted"
        );
        assert_eq!(redact_text("operator stop"), "operator stop");
    }

    #[test]
    fn auth_store_type_remains_constructible_for_public_engine_api() {
        let _ = AuthStore::default();
    }
}
