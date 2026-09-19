use super::super::helpers::{clear_spinner, disarm_spinner, redact_value};
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

struct ReasoningStreamPlan {
    prefix_blank: bool,
    internal_newlines: Option<&'static str>,
    content: String,
}

fn format_reasoning_text(history: &[String], redacted: &str) -> String {
    if let Some(last) = history.last()
        && !last.is_empty()
        && (last.ends_with('.') || last.ends_with('!') || last.ends_with('?'))
        && !redacted.starts_with(' ')
        && !redacted.starts_with('\n')
    {
        format!(" {redacted}")
    } else {
        redacted.to_string()
    }
}

fn pop_internal_newlines(state: &mut TerminalSinkState) -> Option<&'static str> {
    if !state.has_reasoning_content || state.pending_reasoning_newlines == 0 {
        return None;
    }
    let count = state.pending_reasoning_newlines.min(2);
    state.pending_reasoning_newlines = 0;
    Some(if count == 1 { "\n" } else { "\n\n" })
}

fn update_reasoning_state(state: &mut TerminalSinkState, text_to_stream: &str) -> ReasoningStreamPlan {
    state.reasoning.push(text_to_stream.to_string());
    let (content, trailing_newlines) = split_reasoning_chunk(text_to_stream);
    if content.is_empty() {
        state.pending_reasoning_newlines += trailing_newlines;
        return ReasoningStreamPlan {
            prefix_blank: false,
            internal_newlines: None,
            content: String::new(),
        };
    }
    let internal_newlines = pop_internal_newlines(state);
    let prefix_blank = matches!(
        state.last_display,
        DisplayKind::Tool | DisplayKind::Text | DisplayKind::None
    );
    state.last_display = DisplayKind::Thinking;
    state.has_reasoning_content = true;
    state.pending_reasoning_newlines = trailing_newlines;
    ReasoningStreamPlan {
        prefix_blank,
        internal_newlines,
        content: content.to_string(),
    }
}

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
        let collected = self
            .state
            .lock()
            .map(|mut state| {
                if state.reasoning.is_empty() {
                    return None;
                }
                let text = state.reasoning.join("");
                state.reasoning.clear();
                state.pending_reasoning_newlines = 0;
                let had_content = state.has_reasoning_content;
                state.has_reasoning_content = false;
                state.last_display = DisplayKind::Thinking;
                if had_content { Some(text) } else { None }
            })
            .unwrap_or(None);

        if let Some(text) = collected {
            self.presenter.finish_thinking(&text);
            self.presenter.write_output("\n");
        }
    }

    fn ensure_reasoning_spinner(&self) {
        if !self.presenter.has_interactive_ui() {
            self.finish_spinner();
        } else if let Ok(mut state) = self.state.lock()
            && state.spinner.is_none()
        {
            state.spinner = Some(self.presenter.start_spinner("thinking..."));
        }
    }

    fn present_reasoning_plan(&self, plan: ReasoningStreamPlan) {
        if plan.prefix_blank {
            self.presenter.write_output("\n");
        }
        if let Some(newlines) = plan.internal_newlines {
            self.presenter.print_thinking_token(newlines);
        }
        if !plan.content.is_empty() {
            self.presenter.print_thinking_token(&plan.content);
        }
    }

    pub fn emit_reasoning(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.ensure_reasoning_spinner();
        let plan = {
            let Ok(mut state) = self.state.lock() else { return };
            let redacted = self.session_manager.redact_credentials(text);
            let text_to_stream = format_reasoning_text(&state.reasoning, &redacted);
            update_reasoning_state(&mut state, &text_to_stream)
        };
        self.present_reasoning_plan(plan);
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
            disarm_spinner(&mut state);
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

    fn finish_tool_presentation(
        &self,
        details: &ToolFinishDetails<'_>,
        arguments: &Value,
        output: &str,
        duration_ms: Option<u64>,
    ) {
        self.presenter.finish_tool_line(ToolLine {
            name: details.name.to_string(),
            arguments: arguments.clone(),
            is_error: details.is_error,
            output: output.to_string(),
            output_summary: summarize_tool_output(output),
            duration_ms,
        });
    }

    fn record_completed_tool(&self, state: &mut TerminalSinkState, details: ToolFinishDetails<'_>, status: &str) {
        disarm_spinner(state);
        state.last_display = DisplayKind::Tool;
        let duration_ms = state
            .pending
            .remove(details.name)
            .and_then(|p| p.started)
            .map(|s| s.elapsed().as_millis() as u64);
        let arguments = redact_value(&self.session_manager, details.arguments);
        let output_redacted = self.session_manager.redact_credentials(details.output);
        self.finish_tool_presentation(&details, &arguments, &output_redacted, duration_ms);
        state.completed.push(CompletedTool {
            internal_call_id: uuid::Uuid::new_v4().to_string(),
            name: details.name.to_string(),
            arguments,
            output: output_redacted,
            status: status.to_string(),
        });
        state.spinner = Some(self.presenter.start_spinner("thinking..."));
    }

    pub fn tool_finished(&self, details: ToolFinishDetails<'_>) {
        let status = if details.is_error { "error" } else { "success" };
        self.run_tracker.tool_finished(status);
        self.flush_reasoning();
        self.presenter.flush();
        if let Ok(mut state) = self.state.lock() {
            self.record_completed_tool(&mut state, details, status);
        }
    }

    pub fn tool_chunk(&self, chunk: &str) {
        self.presenter.stream_port().stream_chunk(chunk);
    }
}

impl Drop for TerminalApprovalSink {
    fn drop(&mut self) {
        self.flush_reasoning();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use rho_harness_core::presentation::ActivityToken;
    use rho_harness_core::presentation::activity::activity_token;
    use rho_harness_core::presentation::stream::ToolStreamPort;
    use rho_harness_core::presentation::{SessionStatus, WelcomeDisplay};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TrackingPresenter {
        idle_count: Arc<AtomicUsize>,
        active_tools: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Presenter for TrackingPresenter {
        fn write_output(&self, _text: &str) {}
        fn print_welcome(&self, _display: &WelcomeDisplay) {}
        fn print_session_status(&self, _display: &SessionStatus) {}
        fn print_notice(&self, _text: &str) {}
        fn print_user_block(&self, _input: &str) {}
        fn print_token(&self, _token: &str) {}
        fn print_thinking_token(&self, _token: &str) {}
        fn finish_tool_line(&self, _line: ToolLine) {}
        fn flush(&self) {}
        fn has_interactive_ui(&self) -> bool {
            true
        }
        fn start_spinner(&self, _message: &str) -> ActivityToken {
            let idles = self.idle_count.clone();
            activity_token(move || {
                idles.fetch_add(1, Ordering::SeqCst);
            })
        }
        fn start_tool_spinner(&self, _name: &str, _arguments: &Value) -> ActivityToken {
            let idles = self.idle_count.clone();
            activity_token(move || {
                idles.fetch_add(1, Ordering::SeqCst);
            })
        }
        fn start_tool_run(&self, _name: &str, _arguments: &Value) {
            self.active_tools.fetch_add(1, Ordering::SeqCst);
        }
        fn stream_port(&self) -> ToolStreamPort {
            ToolStreamPort::default()
        }
    }

    #[test]
    fn tool_transitions_disarm_intermediate_spinners_without_emitting_idle() {
        let temp = tempfile::tempdir().unwrap();
        let session = rho_harness_core::session::SessionManager::new(temp.path(), None).unwrap();
        let idle_count = Arc::new(AtomicUsize::new(0));
        let active_tools = Arc::new(AtomicUsize::new(0));
        let presenter: Arc<dyn Presenter> = Arc::new(TrackingPresenter {
            idle_count: idle_count.clone(),
            active_tools: active_tools.clone(),
        });
        let sink = TerminalApprovalSink::new(
            &presenter,
            TerminalSinkConfig {
                model_label: "test".to_string(),
                run_tracker: crate::engine::metrics::RunTracker::default(),
            },
            session,
        );

        sink.resume_model_spinner();
        assert_eq!(idle_count.load(Ordering::SeqCst), 0);

        sink.tool_start("write", &serde_json::json!({"path": "file.txt", "content": "data"}));
        assert_eq!(idle_count.load(Ordering::SeqCst), 0);
        assert_eq!(active_tools.load(Ordering::SeqCst), 1);

        sink.tool_finished(ToolFinishDetails {
            name: "write",
            arguments: &serde_json::json!({"path": "file.txt"}),
            output: "ok",
            is_error: false,
        });
        assert_eq!(idle_count.load(Ordering::SeqCst), 0);

        sink.finish_spinner();
        assert_eq!(idle_count.load(Ordering::SeqCst), 1);
    }
}
