//! Shows a subagent's turn in its monitor record: the same `Presenter` calls the main agent's turn
//! makes to the terminal become the agent's streamed text, thinking, and tool cards.

use rho_harness_core::agents::{AgentEvent, AgentHandle, Streamed};
use rho_harness_core::presentation::activity::activity_token;
use rho_harness_core::presentation::{
    ActivityToken, Presenter, SessionStatus, ToolLine, ToolStreamPort, WelcomeDisplay,
};
use serde_json::Value;

pub(crate) struct AgentPresenter {
    tracker: Option<AgentHandle>,
    progress: ToolStreamPort,
}

impl AgentPresenter {
    pub(crate) fn new(tracker: Option<AgentHandle>, progress: ToolStreamPort) -> Self {
        Self { tracker, progress }
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

    fn finish_tool_line(&self, line: ToolLine) {
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
        self.event(AgentEvent::ToolCall {
            name: name.to_string(),
            args: arguments.to_string(),
        });
    }

    fn stream_port(&self) -> ToolStreamPort {
        self.progress.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_harness_core::agents::AgentMonitor;

    #[test]
    fn the_turns_output_becomes_the_agents_record() {
        let monitor = AgentMonitor::default();
        let handle = monitor.start("g", "a", "m", 3);
        let presenter = AgentPresenter::new(Some(handle.clone()), ToolStreamPort::default());
        let args = serde_json::json!({ "path": "a.rs" });

        presenter.print_user_block("look at a.rs");
        presenter.print_thinking_token("hmm");
        presenter.print_token("Reading");
        presenter.start_tool_run("read", &args);
        presenter.finish_tool_line(ToolLine {
            name: "read".into(),
            arguments: args.clone(),
            is_error: false,
            output: "fn a() {}".into(),
            output_summary: "fn a() {}".into(),
            duration_ms: Some(3),
            file_change: None,
        });

        let events = monitor.record(handle.id()).unwrap().events;
        assert_eq!(events.len(), 4, "{events:?}");
        assert_eq!(events[0], AgentEvent::Task("look at a.rs".into()));
        assert_eq!(events[1], AgentEvent::Thinking("hmm".into()));
        assert_eq!(events[2], AgentEvent::Text("Reading".into()));
        assert!(matches!(&events[3], AgentEvent::Tool(line) if line.output == "fn a() {}"));
    }
}
