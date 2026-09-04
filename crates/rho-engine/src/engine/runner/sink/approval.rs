use super::super::helpers::{clear_spinner, redact_value};
use super::types::{CompletedTool, DisplayKind, PendingToolCall, TerminalSinkConfig};
use crate::engine::metrics::RunTracker;
use rho_harness_core::presentation::{Presenter, ToolLine, summarize_tool_output};
use rho_harness_core::session::SessionManager;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

pub struct TerminalSinkState {
    pub auto_approve: bool,
    pub spinner: Option<rho_harness_core::presentation::ActivityToken>,
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

pub struct ToolFinishDetails<'a> {
    pub name: &'a str,
    pub arguments: &'a Value,
    pub output: &'a str,
    pub is_error: bool,
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
            if state.last_display == DisplayKind::Tool
                || state.last_display == DisplayKind::Text
                || state.last_display == DisplayKind::None
            {
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
        if !self.presenter.has_interactive_ui() {
            self.finish_spinner();
        }
        self.flush_reasoning();
        let mut prefix_blank = false;
        if let Ok(mut state) = self.state.lock() {
            if state.last_display == DisplayKind::Tool
                || state.last_display == DisplayKind::Thinking
                || state.last_display == DisplayKind::None
            {
                prefix_blank = true;
            }
            state.last_display = DisplayKind::Text;
        }
        if prefix_blank {
            self.presenter.write_output("\n");
        }
        let redacted = self.session_manager.redact_credentials(text);
        self.presenter.print_token(&redacted);
    }

    pub fn flush_display(&self) {
        self.flush_reasoning();
        self.presenter.flush();
    }

    pub fn tool_start(&self, name: &str, arguments: &Value) {
        self.run_tracker.tool_called();
        self.flush_reasoning();
        self.presenter.flush();
        let arguments = redact_value(&self.session_manager, arguments);
        if let Ok(mut state) = self.state.lock() {
            clear_spinner(&mut state);
            state.pending.insert(
                name.to_string(),
                PendingToolCall {
                    name: name.to_string(),
                    arguments: arguments.clone(),
                    started: Some(Instant::now()),
                },
            );
            state.spinner = Some(self.presenter.start_tool_spinner(name, &arguments));
            self.presenter.start_tool_run(name, &arguments);
        }
    }

    pub fn tool_finished(&self, details: ToolFinishDetails<'_>) {
        let status = if details.is_error { "error" } else { "success" };
        self.run_tracker.tool_finished(status);
        if let Ok(mut state) = self.state.lock() {
            clear_spinner(&mut state);
            state.last_display = DisplayKind::Tool;
            let duration_ms = state
                .pending
                .remove(details.name)
                .and_then(|p| p.started)
                .map(|s| s.elapsed().as_millis() as u64);
            let arguments = redact_value(&self.session_manager, details.arguments);
            let output_redacted = self.session_manager.redact_credentials(details.output);
            let output_summary = summarize_tool_output(&output_redacted);
            self.presenter.finish_tool_line(ToolLine {
                name: details.name.to_string(),
                arguments: arguments.clone(),
                is_error: details.is_error,
                output: output_redacted.clone(),
                output_summary,
                duration_ms,
            });
            state.completed.push(CompletedTool {
                internal_call_id: uuid::Uuid::new_v4().to_string(),
                name: details.name.to_string(),
                arguments,
                output: output_redacted,
                status: status.to_string(),
            });
            state.spinner = Some(self.presenter.start_spinner("thinking..."));
        }
    }
}
