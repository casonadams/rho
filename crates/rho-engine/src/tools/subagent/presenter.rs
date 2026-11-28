//! Shows a subagent's turn in its monitor record: the same `Presenter` calls the main agent's turn
//! makes to the terminal become the agent's streamed text, thinking, and tool cards.

use crate::tools::file_change::PendingFileChanges;
use rho_harness_core::agents::{AgentEvent, AgentHandle, Streamed};
use rho_harness_core::presentation::activity::activity_token;
use rho_harness_core::presentation::stream::ToolStreamSink;
use rho_harness_core::presentation::{
    ActivityToken, Presenter, SessionStatus, ToolLine, ToolStreamPort, WelcomeDisplay,
};
use serde_json::Value;
use std::sync::Arc;

/// Output streamed to a call's card while the model writes it, kept on the agent's record.
struct CallOutput(AgentHandle);

impl ToolStreamSink for CallOutput {
    fn tool_chunk(&self, chunk: String) {
        self.0.call_output(&chunk);
    }
}

/// Presents a turn into an agent's monitor record instead of the terminal.
pub struct AgentPresenter {
    tracker: Option<AgentHandle>,
    file_changes: PendingFileChanges,
}

impl AgentPresenter {
    pub fn new(tracker: Option<AgentHandle>) -> Self {
        Self {
            tracker,
            file_changes: PendingFileChanges::default(),
        }
    }

    fn event(&self, event: AgentEvent) {
        if let Some(tracker) = &self.tracker {
            tracker.event(event);
        }
    }

    fn stream(&self, kind: Streamed, token: &str) {
        if let Some(tracker) = &self.tracker {
            tracker.stream(kind, token);
        }
    }
}

#[async_trait::async_trait]
impl Presenter for AgentPresenter {
    fn write_output(&self, _text: &str) {}
    fn print_welcome(&self, _display: &WelcomeDisplay) {}
    fn print_session_status(&self, _display: &SessionStatus) {}
    fn print_notice(&self, _text: &str) {}

    fn print_user_block(&self, input: &str) {
        self.event(AgentEvent::Task(input.to_string()));
    }

    fn print_token(&self, token: &str) {
        self.stream(Streamed::Text, token);
    }

    fn print_thinking_token(&self, token: &str) {
        self.stream(Streamed::Thinking, token);
    }

    fn finish_tool_line(&self, mut line: ToolLine) {
        self.file_changes.attach(&mut line);
        self.event(AgentEvent::Tool(line));
    }

    fn flush(&self) {}

    fn has_interactive_ui(&self) -> bool {
        true
    }

    fn start_spinner(&self, _message: &str) -> ActivityToken {
        activity_token(|| {})
    }

    fn start_tool_spinner(&self, _name: &str, _arguments: &Value) -> ActivityToken {
        activity_token(|| {})
    }

    fn start_tool_run(&self, name: &str, arguments: &Value) {
        let file_change = self.file_changes.note(name, arguments);
        self.event(AgentEvent::tool_call(name, arguments.clone(), file_change));
    }

    fn preview_tool_run(&self, name: &str, arguments: &Value) {
        let file_change = self.file_changes.note_preview(name, arguments);
        if let Some(tracker) = &self.tracker {
            tracker.preview_call(name, arguments.clone(), file_change);
        }
    }

    fn end_tool_run(&self) {
        self.file_changes.end_preview();
        if let Some(tracker) = &self.tracker {
            tracker.end_call();
        }
    }

    fn stream_port(&self) -> ToolStreamPort {
        let sink = self
            .tracker
            .clone()
            .map(|tracker| Arc::new(CallOutput(tracker)) as Arc<dyn ToolStreamSink>);
        ToolStreamPort::new(sink)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_harness_core::agents::AgentMonitor;
    use std::sync::Arc;

    #[test]
    fn the_turns_output_becomes_the_agents_record() {
        let monitor = AgentMonitor::default();
        let handle = monitor.start("g", "a", "m", 3);
        let presenter = AgentPresenter::new(Some(handle.clone()));
        let args = serde_json::json!({ "path": "a.rs" });

        presenter.print_user_block("look at a.rs");
        presenter.print_thinking_token("hmm");
        presenter.print_token("Reading");
        presenter.start_tool_run("read", &args);
        presenter.finish_tool_line(
            ToolLine::new("read", args.clone())
                .with_output("fn a() {}")
                .with_output_summary("fn a() {}")
                .with_duration_ms(3),
        );

        let events = monitor.record(handle.id()).unwrap().events;
        assert_eq!(events.len(), 4, "{events:?}");
        assert_eq!(events[0], AgentEvent::Task("look at a.rs".into()));
        assert_eq!(events[1], AgentEvent::Thinking("hmm".into()));
        assert_eq!(events[2], AgentEvent::Text("Reading".into()));
        assert!(matches!(&events[3], AgentEvent::Tool(line) if line.output == "fn a() {}"));
    }

    /// A turn's sink presenting to `handle`'s record, as a subagent round builds it.
    fn agent_sink(
        handle: &rho_harness_core::agents::AgentHandle,
    ) -> (
        Arc<dyn Presenter>,
        Arc<crate::engine::runner::sink::TerminalApprovalSink>,
        crate::test_support::TestDir,
    ) {
        let presenter: Arc<dyn Presenter> = Arc::new(AgentPresenter::new(Some(handle.clone())));
        let dir = crate::test_support::TestDir::new();
        let session = rho_harness_core::session::SessionManager::new(&dir, None).unwrap();
        let config = crate::engine::runner::sink::TerminalSinkConfig {
            run_tracker: crate::engine::metrics::RunTracker::default(),
        };
        let sink = crate::engine::runner::sink::TerminalApprovalSink::new(&presenter, config, session);
        (presenter, sink, dir)
    }

    #[test]
    fn a_write_streams_into_the_agents_record_and_is_replaced_by_its_finished_line() {
        let monitor = AgentMonitor::default();
        let handle = monitor.start("g", "a", "m", 3);
        let (presenter, sink, _dir) = agent_sink(&handle);
        let args = serde_json::json!({ "path": "a.txt", "text": "hello world" });

        sink.preview_tool("call-1", "write", &serde_json::json!({}));
        sink.preview_tool_chunk("hello");
        sink.preview_tool("call-1", "write", &serde_json::json!({ "path": "a.txt" }));
        sink.preview_tool_chunk("hello wor");

        let streaming = monitor.record(handle.id()).unwrap().events;
        let [
            AgentEvent::ToolCall {
                args: shown, output, ..
            },
        ] = &streaming[..]
        else {
            panic!("the preview is one call: {streaming:?}");
        };
        assert_eq!(
            (shown, output.as_str()),
            (&serde_json::json!({ "path": "a.txt" }), "hello wor")
        );

        sink.tool_start("write", &args);
        presenter.finish_tool_line(ToolLine::new("write", args.clone()));
        let finished = monitor.record(handle.id()).unwrap().events;
        assert!(
            matches!(&finished[..], [AgentEvent::Tool(line)] if line.arguments == args),
            "{finished:?}"
        );
    }

    #[test]
    fn a_cancelled_preview_leaves_no_pending_call_behind() {
        let monitor = AgentMonitor::default();
        let handle = monitor.start("g", "a", "m", 3);
        let (presenter, sink, _dir) = agent_sink(&handle);

        sink.preview_tool("call-1", "write", &serde_json::json!({ "path": "a.txt" }));
        handle.stream(rho_harness_core::agents::Streamed::Text, "retrying");
        sink.cancel_tool_preview();
        let args = serde_json::json!({ "path": "a.txt", "text": "hi" });
        sink.preview_tool("call-2", "write", &serde_json::json!({ "path": "a.txt" }));
        sink.tool_start("write", &args);
        presenter.finish_tool_line(ToolLine::new("write", args.clone()));

        let events = monitor.record(handle.id()).unwrap().events;
        assert!(
            matches!(&events[..], [AgentEvent::Text(_), AgentEvent::Tool(line)] if line.arguments == args),
            "{events:?}"
        );
    }

    #[test]
    fn a_finished_write_carries_the_state_its_file_had_when_it_started() {
        let dir = crate::test_support::TestDir::new();
        let file = dir.join("f.txt");
        std::fs::write(&file, "old").unwrap();
        let monitor = AgentMonitor::default();
        let handle = monitor.start("g", "a", "m", 3);
        let presenter = AgentPresenter::new(Some(handle.clone()));
        let args = serde_json::json!({ "path": file.to_str().unwrap(), "text": "new" });

        presenter.start_tool_run("write", &args);
        std::fs::write(&file, "new").unwrap();
        presenter.finish_tool_line(ToolLine::new("write", args));

        let events = monitor.record(handle.id()).unwrap().events;
        let [AgentEvent::Tool(line)] = &events[..] else {
            panic!("expected the finished write: {events:?}");
        };
        assert!(
            line.file_change
                .as_ref()
                .is_some_and(|change| change.overwrote_existing)
        );
    }
}
