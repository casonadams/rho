use crate::engine::metrics::RunTracker;
use async_trait::async_trait;
use rho_core::approval::{ApprovalEventSink, ApprovalRequest, ToolEvent};
use rho_core::presentation::{ApprovalResult, BashApproval, ToolLine};
use rho_core::presentation::{Presenter, summarize_tool_output};
use rho_core::session::SessionManager;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use super::helpers::{clear_spinner, needs_approval, redact_value};
use super::history::DisplayEvent;

#[derive(Clone)]
pub struct CompletedTool {
    pub internal_call_id: String,
    pub name: String,
    pub arguments: Value,
    pub output: String,
    pub status: String,
}

pub struct TurnArtifacts {
    pub response: rig::agent::PromptResponse,
    pub tool_calls_count: usize,
    pub completed_tools: Vec<CompletedTool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplayKind {
    #[default]
    None,
    Thinking,
    Text,
    Tool,
}

pub struct PendingToolCall {
    pub name: String,
    pub arguments: Value,
    pub started: Option<Instant>,
}

pub struct TerminalSinkState {
    pub auto_approve: bool,
    pub spinner: Option<rho_core::presentation::ActivityToken>,
    pub pending: HashMap<String, PendingToolCall>,
    pub reasoning: Vec<String>,
    pub completed: Vec<CompletedTool>,
    pub last_display: DisplayKind,
}

pub struct TerminalApprovalSink {
    pub presenter: std::sync::Arc<dyn Presenter>,
    pub model_label: String,
    pub session_manager: SessionManager,
    pub run_tracker: RunTracker,
    pub state: Mutex<TerminalSinkState>,
}

pub struct TerminalSinkConfig {
    pub model_label: String,
    pub auto_approve: bool,
    pub run_tracker: RunTracker,
}

impl TerminalApprovalSink {
    pub fn new(
        presenter: &std::sync::Arc<dyn Presenter>,
        config: TerminalSinkConfig,
        session_manager: SessionManager,
    ) -> std::sync::Arc<Self> {
        let spinner = presenter.start_spinner("thinking...");
        std::sync::Arc::new(Self {
            presenter: std::sync::Arc::clone(presenter),
            model_label: config.model_label,
            session_manager,
            run_tracker: config.run_tracker,
            state: Mutex::new(TerminalSinkState {
                auto_approve: config.auto_approve,
                spinner: Some(spinner),
                pending: HashMap::new(),
                reasoning: Vec::new(),
                completed: Vec::new(),
                last_display: DisplayKind::None,
            }),
        })
    }

    pub fn finish_spinner(&self) {
        if let Ok(mut state) = self.state.lock() {
            clear_spinner(&mut state);
        }
    }

    pub fn resume_model_spinner(&self) {
        if self.state.lock().is_ok_and(|state| state.spinner.is_some()) {
            return;
        }
        self.flush_reasoning();
        self.presenter.flush();
        if let Ok(mut state) = self.state.lock()
            && state.spinner.is_none()
        {
            state.spinner = Some(self.presenter.start_spinner("thinking..."));
        }
    }

    pub fn completed(&self) -> Vec<CompletedTool> {
        self.state
            .lock()
            .map(|state| state.completed.clone())
            .unwrap_or_default()
    }

    pub fn flush_reasoning(&self) {
        let had_reasoning = self
            .state
            .lock()
            .map(|mut state| {
                if state.reasoning.is_empty() {
                    return false;
                }
                state.reasoning.clear();
                state.last_display = DisplayKind::Thinking;
                true
            })
            .unwrap_or(false);

        if had_reasoning {
            self.presenter.write_output("\n");
        }
    }

    pub fn emit_reasoning(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        if !self.presenter.has_interactive_ui() {
            self.finish_spinner();
        } else if self.state.lock().is_ok_and(|state| state.spinner.is_none())
            && let Ok(mut state) = self.state.lock()
        {
            state.spinner = Some(self.presenter.start_spinner("thinking..."));
        }
        let mut prefix_blank = false;
        let mut text_to_stream = text.to_string();

        if let Ok(mut state) = self.state.lock() {
            let redacted = self.session_manager.redact_credentials(text);
            if state.last_display == DisplayKind::Tool || state.last_display == DisplayKind::Text {
                prefix_blank = true;
            }
            state.last_display = DisplayKind::Thinking;

            if let Some(last) = state.reasoning.last()
                && !last.is_empty()
                && (last.ends_with('.') || last.ends_with('!') || last.ends_with('?'))
                && !redacted.starts_with(' ')
                && !redacted.starts_with('\n')
            {
                text_to_stream = format!(" {redacted}");
                state.reasoning.push(text_to_stream.clone());
            } else {
                text_to_stream = redacted.clone();
                state.reasoning.push(redacted);
            }
        }

        if prefix_blank {
            self.presenter.write_output("\n");
        }
        self.presenter.print_thinking_token(&text_to_stream);
    }

    pub fn emit_text(&self, text: &str) {
        self.flush_reasoning();
        if !self.presenter.has_interactive_ui() {
            self.finish_spinner();
        }
        if let Ok(mut state) = self.state.lock() {
            if self.presenter.has_interactive_ui() && state.last_display != DisplayKind::Text {
                clear_spinner(&mut state);
                state.spinner = Some(self.presenter.start_spinner("working..."));
            }
            if state.last_display == DisplayKind::Tool || state.last_display == DisplayKind::Thinking {
                self.presenter.write_output("\n");
            }
            state.last_display = DisplayKind::Text;
        }
        self.presenter
            .print_token(&self.session_manager.redact_credentials(text));
    }

    pub fn flush_display(&self) {
        self.flush_reasoning();
        self.presenter.flush();
        if let Ok(state) = self.state.lock()
            && state.last_display == DisplayKind::Tool
        {
            self.presenter.write_output("\n");
        }
    }

    pub fn display_event(&self, event: &DisplayEvent) {
        match event {
            DisplayEvent::Text(text) => self.emit_text(text),
            DisplayEvent::Reasoning(text) => self.emit_reasoning(text),
            DisplayEvent::ToolCall => {
                self.flush_reasoning();
            }
        }
    }
}

#[async_trait]
impl ApprovalEventSink for TerminalApprovalSink {
    async fn request_approval(&self, request: ApprovalRequest) -> rho_core::approval::ApprovalDecision {
        self.finish_spinner();
        self.flush_reasoning();
        self.presenter.flush();
        let arguments = redact_value(&self.session_manager, &request.arguments);

        let result = if request.tool_name == "bash" {
            let command = arguments.get("command").and_then(Value::as_str).unwrap_or_default();
            self.presenter
                .prompt_bash_approval(BashApproval {
                    command: command.to_string(),
                    tier: request.tier,
                    reasons: request.reasons.clone(),
                })
                .await
        } else {
            self.presenter
                .prompt_tool_approval(&request.tool_name, &arguments)
                .await
        };
        match result {
            ApprovalResult::Approved => rho_core::approval::ApprovalDecision::Approved,
            ApprovalResult::ApprovedForSession => rho_core::approval::ApprovalDecision::ApprovedForSession,
            ApprovalResult::Denied { reason } => rho_core::approval::ApprovalDecision::Denied { reason },
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
                self.presenter.flush();
                let arguments = redact_value(&self.session_manager, &arguments);
                let mut state = self.state.lock().unwrap_or_else(|_| unreachable!());
                clear_spinner(&mut state);
                let runs_immediately =
                    !needs_approval(&state, &class) && tool_name != "ask_user" && tool_name != "ask_user_question";
                if runs_immediately {
                    state.spinner = Some(self.presenter.start_tool_spinner(&tool_name, &arguments));
                    self.presenter.start_tool_run(&tool_name, &arguments);
                }
                state.pending.insert(
                    internal_call_id,
                    PendingToolCall {
                        name: tool_name,
                        arguments,
                        started: runs_immediately.then(Instant::now),
                    },
                );
            }
            ToolEvent::ApprovalGranted { internal_call_id, .. } => {
                if let Ok(mut state) = self.state.lock()
                    && let Some(call) = state.pending.get_mut(&internal_call_id)
                    && call.name != "ask_user"
                    && call.name != "ask_user_question"
                {
                    call.started = Some(Instant::now());
                    let (name, arguments) = (call.name.clone(), call.arguments.clone());
                    clear_spinner(&mut state);
                    state.spinner = Some(self.presenter.start_tool_spinner(&name, &arguments));
                    self.presenter.start_tool_run(&name, &arguments);
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
                    let finished = state.pending.remove(&internal_call_id);
                    let duration = finished.and_then(|call| call.started).map(|started| started.elapsed());
                    clear_spinner(&mut state);
                    state.last_display = DisplayKind::Tool;
                    let arguments = redact_value(&self.session_manager, &arguments);
                    let output = self.session_manager.redact_credentials(&output);
                    let output_summary = summarize_tool_output(&output);
                    self.presenter.finish_tool_line(ToolLine {
                        name: tool_name.clone(),
                        arguments: arguments.clone(),
                        is_error: status != "success",
                        output: output.clone(),
                        output_summary: output_summary.clone(),
                        duration_ms: duration.map(|d| d.as_millis() as u64),
                    });
                    state.completed.push(CompletedTool {
                        internal_call_id,
                        name: tool_name,
                        arguments,
                        output,
                        status,
                    });
                    state.spinner = Some(self.presenter.start_spinner("thinking..."));
                }
            }
        }
    }
}
