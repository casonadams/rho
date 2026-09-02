use super::renderer::TerminalRenderer;
use crate::ui::interactive::InteractiveUi;
use async_trait::async_trait;
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::stream::{ToolStreamPort, ToolStreamSink};
use rho_harness_core::presentation::summary::format_tool_args_full;
use rho_harness_core::presentation::{ActivityToken, activity_token};
use rho_harness_core::presentation::{
    ApprovalResult, BashApproval, InteractionPrompt, InteractionResponse, SessionStatus, ToolLine, WelcomeDisplay,
};
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

    async fn prompt_tool_approval(&self, name: &str, arguments: &Value) -> ApprovalResult {
        if let Some(ui) = &self.ui {
            let prompt = crate::ui::interactive::InteractionPrompt {
                title: "Permission Request".to_string(),
                body: approval_body(name, arguments),
                options: vec![
                    crate::ui::interactive::InteractionOption {
                        label: "Allow".to_string(),
                        description: Some("Execute this tool call".to_string()),
                    },
                    crate::ui::interactive::InteractionOption {
                        label: "Deny".to_string(),
                        description: Some("Block this tool call".to_string()),
                    },
                ],
                initial_selection: 0,
                allow_custom: false,
            };

            match ui.request(prompt).await {
                Ok(crate::ui::interactive::InteractionResponse::Selected(0)) => ApprovalResult::Approved,
                _ => ApprovalResult::Denied {
                    reason: "user rejected tool execution".to_string(),
                },
            }
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

/// The modal body: the tool's input verbatim (no JSON braces) for known
/// tools; compact JSON capped at 200 chars for unknown (e.g. MCP) tools.
fn approval_body(name: &str, arguments: &Value) -> String {
    match input_summary(name, arguments) {
        Some(summary) => format!("Tool: {name}\n{summary}"),
        None => format!("Tool: {name}"),
    }
}

fn input_summary(name: &str, arguments: &Value) -> Option<String> {
    let summary = format_tool_args_full(name, arguments);
    if !summary.is_empty() {
        return Some(summary);
    }
    if arguments.as_object().is_none_or(|object| object.is_empty()) {
        return None;
    }
    let text = serde_json::to_string(arguments).unwrap_or_default();
    let capped = if text.chars().count() <= 200 {
        text
    } else {
        format!("{}\u{2026}", text.chars().take(200).collect::<String>())
    };
    (!capped.is_empty()).then_some(capped)
}
