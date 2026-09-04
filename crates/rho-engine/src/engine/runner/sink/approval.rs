use super::super::helpers::{clear_spinner, redact_value};
use super::reasoning::split_reasoning_chunk;
use super::types::{
    CompletedTool, DisplayKind, PendingToolCall, TerminalSinkConfig, TerminalSinkState, ToolFinishDetails,
};
use crate::engine::metrics::RunTracker;
use rho_harness_core::presentation::{Presenter, ToolLine, summarize_tool_output};
use rho_harness_core::session::SessionManager;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

pub struct TerminalApprovalSink {
    pub presenter: std::sync::Arc<dyn Presenter>,
    pub model_label: String,
    pub session_manager: SessionManager,
    pub run_tracker: RunTracker,
    pub state: Mutex<TerminalSinkState>,
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
                pending_reasoning_newlines: 0,
                has_reasoning_content: false,
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
        let had_content = self
            .state
            .lock()
            .map(|mut state| {
                if state.reasoning.is_empty() {
                    return false;
                }
                state.reasoning.clear();
                state.pending_reasoning_newlines = 0;
                let had_content = state.has_reasoning_content;
                state.has_reasoning_content = false;
                state.last_display = DisplayKind::Thinking;
                had_content
            })
            .unwrap_or(false);

        if had_content {
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
        let mut internal_newlines = None;
        let mut content_to_stream = String::new();

        if let Ok(mut state) = self.state.lock() {
            let redacted = self.session_manager.redact_credentials(text);
            let text_to_stream = if let Some(last) = state.reasoning.last()
                && !last.is_empty()
                && (last.ends_with('.') || last.ends_with('!') || last.ends_with('?'))
                && !redacted.starts_with(' ')
                && !redacted.starts_with('\n')
            {
                format!(" {redacted}")
            } else {
                redacted.clone()
            };
            state.reasoning.push(text_to_stream.clone());

            let (content, trailing_newlines) = split_reasoning_chunk(&text_to_stream);
            if !content.is_empty() {
                if state.has_reasoning_content && state.pending_reasoning_newlines > 0 {
                    let count = state.pending_reasoning_newlines.min(2);
                    internal_newlines = Some(if count == 1 { "\n" } else { "\n\n" });
                    state.pending_reasoning_newlines = 0;
                }
                if state.last_display == DisplayKind::Tool
                    || state.last_display == DisplayKind::Text
                    || state.last_display == DisplayKind::None
                {
                    prefix_blank = true;
                }
                state.last_display = DisplayKind::Thinking;
                state.has_reasoning_content = true;
                state.pending_reasoning_newlines = trailing_newlines;
                content_to_stream = content.to_string();
            } else {
                state.pending_reasoning_newlines += trailing_newlines;
            }
        }

        if prefix_blank {
            self.presenter.write_output("\n");
        }
        if let Some(newlines) = internal_newlines {
            self.presenter.print_thinking_token(newlines);
        }
        if !content_to_stream.is_empty() {
            self.presenter.print_thinking_token(&content_to_stream);
        }
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

    pub fn tool_chunk(&self, chunk: &str) {
        self.presenter.stream_port().stream_chunk(chunk);
    }
}
