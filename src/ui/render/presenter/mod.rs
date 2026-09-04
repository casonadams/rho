//! `Presenter` implementation bridging `TerminalRenderer` to harness core.

mod approval;
mod sink;

pub use sink::InteractiveStreamSink;

use super::renderer::TerminalRenderer;
use approval::prompt_interactive_tool_approval;
use async_trait::async_trait;
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::stream::{ToolStreamPort, ToolStreamSink};
use rho_harness_core::presentation::{ActivityToken, activity_token};
use rho_harness_core::presentation::{
    ApprovalResult, BashApproval, InteractionPrompt, InteractionResponse, SessionStatus, ToolLine, WelcomeDisplay,
};
use serde_json::Value;

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

    async fn prompt_tool_approval(&self, name: &str, arguments: &Value) -> ApprovalResult {
        if let Some(ui) = &self.ui {
            prompt_interactive_tool_approval(ui, name, arguments).await
        } else {
            ApprovalResult::Approved
        }
    }

    async fn request_interaction(&self, prompt: InteractionPrompt) -> Option<InteractionResponse> {
        self.ui.as_ref()?.request(prompt).await.ok()
    }

    fn print_block(&self, display: &rho_harness_core::presentation::BlockDisplay) {
        TerminalRenderer::print_block(self, display);
    }

    fn set_extra_status(&self, status: Option<String>) {
        TerminalRenderer::set_extra_status(self, status);
    }

    async fn prompt_bash_approval(&self, _request: BashApproval) -> ApprovalResult {
        ApprovalResult::Approved
    }

    async fn prompt_continue_budget(&self, _max_turns: usize) -> bool {
        false
    }
}
