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
    /// Supplies the secrets to redact and the log tool events go to; absent for an agent without a session.
    pub session_manager: Option<SessionManager>,
    pub run_tracker: RunTracker,
    pub state: Mutex<TerminalSinkState>,
}

impl TerminalApprovalSink {
    pub fn new(
        presenter: &std::sync::Arc<dyn Presenter>,
        config: TerminalSinkConfig,
        session_manager: SessionManager,
    ) -> std::sync::Arc<Self> {
        Self::with_session(presenter, config, Some(session_manager))
    }

    pub fn with_session(
        presenter: &std::sync::Arc<dyn Presenter>,
        config: TerminalSinkConfig,
        session_manager: Option<SessionManager>,
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
                recorded_events: 0,
                last_display: DisplayKind::None,
                pending_reasoning_newlines: 0,
                has_reasoning_content: false,
                text_carry: String::new(),
                reasoning_carry: String::new(),
                tool_preview: None,
                tool_preview_started: None,
            }),
        })
    }

    pub fn finish_spinner(&self) {
        if let Ok(mut state) = self.state.lock() {
            clear_spinner(&mut state);
        }
    }

    pub fn resume_spinner_with_message(&self, message: &str) {
        self.flush_reasoning();
        self.presenter.flush();
        if let Ok(mut state) = self.state.lock() {
            disarm_spinner(&mut state);
            state.spinner = Some(self.presenter.start_spinner(message));
        }
    }

    pub fn resume_model_spinner(&self) {
        let has_spinner = self.state.lock().is_ok_and(|s| s.spinner.is_some());
        if !has_spinner {
            self.resume_spinner_with_message("thinking...");
        }
    }

    pub fn completed(&self) -> Vec<CompletedTool> {
        self.state
            .lock()
            .map(|state| state.completed.clone())
            .unwrap_or_default()
    }

    pub fn unrecorded(&self) -> Vec<CompletedTool> {
        self.state
            .lock()
            .map(|state| state.completed[state.recorded_events..].to_vec())
            .unwrap_or_default()
    }

    pub async fn record_tool_events(&self) -> rho_harness_core::error::Result<()> {
        let Some(session_manager) = &self.session_manager else {
            return Ok(());
        };
        for tool in self.unrecorded() {
            append_tool_events(session_manager, &tool).await?;
            if let Ok(mut state) = self.state.lock() {
                state.recorded_events += 1;
            }
        }
        Ok(())
    }

    fn redact(&self, text: &str) -> String {
        match &self.session_manager {
            Some(session) => session.redact_credentials(text),
            None => text.to_string(),
        }
    }

    fn redact_streamed(&self, carry: &mut String, chunk: &str) -> String {
        match &self.session_manager {
            Some(session) => session.redact_credentials_streamed(carry, chunk),
            None => chunk.to_string(),
        }
    }

    fn redact_arguments(&self, arguments: &Value) -> Value {
        match &self.session_manager {
            Some(session) => redact_value(session, arguments),
            None => arguments.clone(),
        }
    }

    pub fn flush_reasoning(&self) {
        self.release_text_carry();
        self.finish_reasoning_block();
    }

    fn release_text_carry(&self) {
        let tail = self
            .state
            .lock()
            .map(|mut state| std::mem::take(&mut state.text_carry))
            .unwrap_or_default();
        if !tail.is_empty() {
            self.presenter.print_token(&tail);
        }
    }

    fn release_reasoning_carry(&self) {
        let plan = {
            let Ok(mut state) = self.state.lock() else { return };
            let tail = std::mem::take(&mut state.reasoning_carry);
            if tail.is_empty() {
                return;
            }
            update_reasoning_state(&mut state, &tail)
        };
        self.present_reasoning_plan(plan);
    }

    fn finish_reasoning_block(&self) {
        self.release_reasoning_carry();
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
        self.release_text_carry();
        self.ensure_reasoning_spinner();
        let plan = {
            let Ok(mut state) = self.state.lock() else { return };
            let redacted = self.redact_streamed(&mut state.reasoning_carry, text);
            update_reasoning_state(&mut state, &redacted)
        };
        self.present_reasoning_plan(plan);
    }

    pub fn emit_text(&self, text: &str) {
        if !self.presenter.has_interactive_ui() {
            self.finish_spinner();
        }
        self.finish_reasoning_block();
        let mut prefix_blank = false;
        let mut redacted = String::new();
        if let Ok(mut state) = self.state.lock() {
            if state.last_display == DisplayKind::Tool
                || state.last_display == DisplayKind::Thinking
                || state.last_display == DisplayKind::None
            {
                prefix_blank = true;
            }
            state.last_display = DisplayKind::Text;
            redacted = self.redact_streamed(&mut state.text_carry, text);
        }
        if prefix_blank {
            self.presenter.write_output("\n");
        }
        if !redacted.is_empty() {
            self.presenter.print_token(&redacted);
        }
    }

    pub fn flush_display(&self) {
        self.flush_reasoning();
        self.presenter.flush();
    }

    pub fn tool_start(&self, name: &str, arguments: &Value) {
        self.run_tracker.tool_called();
        self.flush_reasoning();
        self.presenter.flush();
        let arguments = self.redact_arguments(arguments);
        if let Ok(mut state) = self.state.lock() {
            let previewing = state.tool_preview.take().is_some();
            let started = state.tool_preview_started.take().unwrap_or_else(Instant::now);
            disarm_spinner(&mut state);
            state
                .pending
                .entry(name.to_string())
                .or_default()
                .push_back(PendingToolCall {
                    name: name.to_string(),
                    arguments: arguments.clone(),
                    started: Some(started),
                });
            state.spinner = Some(self.presenter.start_tool_spinner(name, &arguments));
            if !previewing {
                self.presenter.start_tool_run(name, &arguments);
            }
        }
    }

    fn finish_tool_presentation(
        &self,
        details: &ToolFinishDetails<'_>,
        arguments: &Value,
        output: &str,
        duration_ms: Option<u64>,
    ) {
        let mut line = ToolLine::new(details.name, arguments.clone())
            .with_output(output)
            .with_output_summary(summarize_tool_output(output));
        if details.is_error {
            line = line.failed();
        }
        line.duration_ms = duration_ms;
        self.presenter.finish_tool_line(line);
    }

    fn record_completed_tool(&self, state: &mut TerminalSinkState, details: ToolFinishDetails<'_>, status: &str) {
        disarm_spinner(state);
        state.last_display = DisplayKind::Tool;
        let duration_ms = state
            .pending
            .get_mut(details.name)
            .and_then(std::collections::VecDeque::pop_front)
            .and_then(|p| p.started)
            .map(|s| s.elapsed().as_millis() as u64);
        let arguments = self.redact_arguments(details.arguments);
        let output_redacted = self.redact(details.output);
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

    /// Shows a card for a tool call whose arguments are still streaming; interactive UIs only.
    pub fn preview_tool(&self, name: &str, arguments: &Value) {
        if !self.presenter.has_interactive_ui() {
            return;
        }
        let arguments = self.redact_arguments(arguments);
        if let Ok(mut state) = self.state.lock() {
            state.tool_preview = Some(String::new());
            state.tool_preview_started.get_or_insert_with(Instant::now);
        }
        self.presenter.start_tool_run(name, &arguments);
    }

    pub fn preview_tool_chunk(&self, chunk: &str) {
        let redacted = {
            let Ok(mut state) = self.state.lock() else { return };
            let Some(mut carry) = state.tool_preview.take() else {
                return;
            };
            let redacted = self.redact_streamed(&mut carry, chunk);
            state.tool_preview = Some(carry);
            redacted
        };
        if !redacted.is_empty() {
            self.presenter.stream_port().stream_chunk(&redacted);
        }
    }

    /// Removes the preview card of a call that will not run.
    pub fn cancel_tool_preview(&self) {
        let previewing = self.state.lock().is_ok_and(|mut state| {
            state.tool_preview_started = None;
            state.tool_preview.take().is_some()
        });
        if previewing {
            self.presenter.end_tool_run();
        }
    }
}

impl Drop for TerminalApprovalSink {
    fn drop(&mut self) {
        self.flush_reasoning();
    }
}

async fn append_tool_events(
    session_manager: &SessionManager,
    tool: &CompletedTool,
) -> rho_harness_core::error::Result<()> {
    use rho_harness_core::session::SessionEventKind;
    session_manager
        .append_event(
            SessionEventKind::ToolCall,
            serde_json::json!({
                "id": tool.internal_call_id,
                "name": tool.name,
                "arguments": tool.arguments,
            }),
        )
        .await?;
    session_manager
        .append_event(
            SessionEventKind::ToolResult,
            serde_json::json!({
                "id": tool.internal_call_id,
                "name": tool.name,
                "output": tool.output,
                "status": tool.status,
            }),
        )
        .await
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
        thinking: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl Presenter for TrackingPresenter {
        fn write_output(&self, _text: &str) {}
        fn print_welcome(&self, _display: &WelcomeDisplay) {}
        fn print_session_status(&self, _display: &SessionStatus) {}
        fn finish_thinking(&self, thinking_text: &str) {
            self.thinking.lock().unwrap().push(thinking_text.to_string());
        }
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

    fn new_test_sink(temp: &tempfile::TempDir, presenter: &Arc<dyn Presenter>) -> Arc<TerminalApprovalSink> {
        let session = rho_harness_core::session::SessionManager::new(temp.path(), None).unwrap();
        TerminalApprovalSink::new(
            presenter,
            TerminalSinkConfig {
                model_label: "test".to_string(),
                run_tracker: crate::engine::metrics::RunTracker::default(),
            },
            session,
        )
    }

    #[test]
    fn tool_transitions_disarm_intermediate_spinners_without_emitting_idle() {
        let temp = tempfile::tempdir().unwrap();
        let idle_count = Arc::new(AtomicUsize::new(0));
        let active_tools = Arc::new(AtomicUsize::new(0));
        let presenter: Arc<dyn Presenter> = Arc::new(TrackingPresenter {
            idle_count: idle_count.clone(),
            active_tools: active_tools.clone(),
            thinking: Arc::default(),
        });
        let sink = new_test_sink(&temp, &presenter);

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

    #[test]
    fn overlapping_calls_to_the_same_tool_each_get_their_own_pending_entry() {
        let temp = tempfile::tempdir().unwrap();
        let presenter: Arc<dyn Presenter> = Arc::new(TrackingPresenter {
            idle_count: Arc::default(),
            active_tools: Arc::default(),
            thinking: Arc::default(),
        });
        let sink = new_test_sink(&temp, &presenter);
        let args = serde_json::json!({"prompt": "p"});
        fn finish<'a>(args: &'a Value, output: &'a str) -> ToolFinishDetails<'a> {
            ToolFinishDetails {
                name: "subagent",
                arguments: args,
                output,
                is_error: false,
            }
        }

        sink.tool_start("subagent", &args);
        sink.tool_start("subagent", &args);
        assert_eq!(sink.state.lock().unwrap().pending["subagent"].len(), 2);

        sink.tool_finished(finish(&args, "one"));
        assert_eq!(sink.state.lock().unwrap().pending["subagent"].len(), 1);
        sink.tool_finished(finish(&args, "two"));

        let state = sink.state.lock().unwrap();
        assert!(state.pending["subagent"].is_empty());
        assert_eq!(state.completed.len(), 2);
    }

    #[test]
    fn reasoning_deltas_are_joined_verbatim() {
        let cases = [
            (["src/ui/thinking.", "rs is next"], "src/ui/thinking.rs is next"),
            (["version 3.", "14"], "version 3.14"),
            (["e.g.", ", this"], "e.g., this"),
            (["Done.", " Next"], "Done. Next"),
        ];
        for (deltas, expected) in cases {
            let temp = tempfile::tempdir().unwrap();
            let thinking = Arc::new(Mutex::new(Vec::new()));
            let presenter: Arc<dyn Presenter> = Arc::new(TrackingPresenter {
                idle_count: Arc::default(),
                active_tools: Arc::default(),
                thinking: thinking.clone(),
            });
            let sink = new_test_sink(&temp, &presenter);
            for delta in deltas {
                sink.emit_reasoning(delta);
            }
            sink.flush_reasoning();
            assert_eq!(*thinking.lock().unwrap(), vec![expected.to_string()]);
        }
    }

    #[test]
    fn secrets_split_across_streamed_chunks_are_redacted() {
        use rho_harness_core::presentation::{RecordingSink, StructuredPresenter, UiEvent};
        let temp = tempfile::tempdir().unwrap();
        let recording = RecordingSink::new();
        let presenter: Arc<dyn Presenter> = Arc::new(StructuredPresenter::recording(recording.clone()));
        let session = rho_harness_core::session::SessionManager::new_with_secrets(
            temp.path(),
            None,
            vec!["credential-sentinel".to_string()],
        )
        .unwrap();
        let sink = TerminalApprovalSink::new(
            &presenter,
            TerminalSinkConfig {
                model_label: "test".to_string(),
                run_tracker: crate::engine::metrics::RunTracker::default(),
            },
            session,
        );

        sink.emit_reasoning("thinking about credential-");
        sink.emit_reasoning("sentinel now");
        sink.emit_text("key: credential-");
        sink.emit_text("sentinel done");
        sink.emit_text(" tail credential-");
        sink.flush_display();

        let events = recording.events();
        let joined = |pick: fn(&UiEvent) -> Option<&str>| events.iter().filter_map(pick).collect::<String>();
        let thinking = joined(|e| match e {
            UiEvent::ThinkingToken { token } => Some(token),
            _ => None,
        });
        let text = joined(|e| match e {
            UiEvent::Token { token } => Some(token),
            _ => None,
        });
        assert_eq!(thinking, "thinking about [REDACTED] now");
        assert_eq!(text, "key: [REDACTED] done tail credential-");
    }

    #[test]
    fn dropping_sink_flushes_buffered_reasoning() {
        let temp = tempfile::tempdir().unwrap();
        let thinking = Arc::new(Mutex::new(Vec::new()));
        let presenter: Arc<dyn Presenter> = Arc::new(TrackingPresenter {
            idle_count: Arc::default(),
            active_tools: Arc::default(),
            thinking: thinking.clone(),
        });
        let sink = new_test_sink(&temp, &presenter);
        sink.emit_reasoning("pondering");
        assert!(thinking.lock().unwrap().is_empty());
        drop(sink);
        assert_eq!(*thinking.lock().unwrap(), vec!["pondering".to_string()]);
    }
}
