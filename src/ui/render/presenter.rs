use super::renderer::TerminalRenderer;
use crate::ui::interactive::InteractiveUi;
use async_trait::async_trait;
use rho_core::presentation::presenter::Presenter;
use rho_core::presentation::questions::QuestionPort;
use rho_core::presentation::stream::{ToolStreamPort, ToolStreamSink};
use rho_core::presentation::{ActivityToken, activity_token};
use rho_core::presentation::{ApprovalResult, BashApproval, SessionStatus, ToolLine, WelcomeDisplay};
use serde_json::Value;

pub struct InteractiveStreamSink(pub Option<InteractiveUi>);

impl ToolStreamSink for InteractiveStreamSink {
    fn tool_chunk(&self, chunk: String) {
        if let Some(ui) = &self.0 {
            let _ = ui.tool_chunk(chunk);
        }
    }
}

#[async_trait]
impl Presenter for TerminalRenderer {
    fn write_output(&self, text: &str) {
        TerminalRenderer::write_output(self, text);
    }

    fn print_welcome(&self, display: &WelcomeDisplay) {
        TerminalRenderer::print_welcome(self, display);
    }

    fn print_session_status(&self, display: &SessionStatus) {
        TerminalRenderer::print_session_status(self, display);
    }

    fn print_notice(&self, text: &str) {
        TerminalRenderer::print_notice(self, text);
    }

    fn print_user_block(&self, input: &str) {
        TerminalRenderer::print_user_block(self, input);
    }

    fn print_token(&self, token: &str) {
        TerminalRenderer::print_token(self, token);
    }

    fn print_thinking_token(&self, token: &str) {
        TerminalRenderer::print_thinking_token(self, token);
    }

    fn finish_tool_line(&self, line: ToolLine) {
        TerminalRenderer::finish_tool_line(self, line);
    }

    fn flush(&self) {
        TerminalRenderer::flush(self);
    }

    fn has_interactive_ui(&self) -> bool {
        TerminalRenderer::has_interactive_ui(self)
    }

    fn start_spinner(&self, message: &str) -> ActivityToken {
        let activity = TerminalRenderer::start_spinner(self, message);
        activity_token(move || activity.finish_and_clear())
    }

    fn start_tool_spinner(&self, name: &str, arguments: &Value) -> ActivityToken {
        let activity = TerminalRenderer::start_tool_spinner(self, name, arguments);
        activity_token(move || activity.finish_and_clear())
    }

    fn start_tool_run(&self, name: &str, arguments: &Value) {
        TerminalRenderer::start_tool_run(self, name, arguments);
    }

    fn stream_port(&self) -> ToolStreamPort {
        ToolStreamPort::new(
            self.ui
                .clone()
                .map(|ui| std::sync::Arc::new(InteractiveStreamSink(Some(ui))) as std::sync::Arc<dyn ToolStreamSink>),
        )
    }

    fn question_port(&self) -> QuestionPort {
        TerminalRenderer::question_port(self)
    }

    async fn prompt_tool_approval(&self, _name: &str, _arguments: &Value) -> ApprovalResult {
        ApprovalResult::Approved
    }

    async fn prompt_bash_approval(&self, _request: BashApproval) -> ApprovalResult {
        ApprovalResult::Approved
    }

    async fn prompt_continue_budget(&self, _max_turns: usize) -> bool {
        false
    }
}
