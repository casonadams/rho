//! A no-op test presenter for offline harness runs; the evaluation suite
//! never observes rendering, so a silent presenter is sufficient.

use std::sync::Arc;

use async_trait::async_trait;
use rho_core::presentation::activity::ActivityToken;
use rho_core::presentation::presenter::Presenter;
use rho_core::presentation::questions::QuestionPort;
use rho_core::presentation::stream::ToolStreamPort;
use rho_core::presentation::types::{ApprovalResult, BashApproval, SessionStatus, ToolLine, WelcomeDisplay};

#[derive(Default)]
pub struct NoopPresenter;

struct NoopQuestionPort;

#[async_trait]
impl rho_core::presentation::questions::InteractiveQuestionPort for NoopQuestionPort {
    async fn ask(
        &self,
        _question: rho_core::presentation::questions::UserQuestion,
    ) -> rho_core::error::Result<rho_core::presentation::questions::UserAnswer> {
        Err(rho_core::error::AppError::Session(
            "no interactive presenter is available in this harness".to_string(),
        ))
    }
}

#[async_trait]
impl Presenter for NoopPresenter {
    fn write_output(&self, _text: &str) {}
    fn print_welcome(&self, _display: &WelcomeDisplay<'_>) {}
    fn print_session_status(&self, _display: &SessionStatus<'_>) {}
    fn print_notice(&self, _text: &str) {}
    fn print_user_block(&self, _input: &str) {}
    fn print_token(&self, _token: &str) {}
    fn print_thinking_token(&self, _token: &str) {}
    fn finish_tool_line(&self, _line: ToolLine<'_>) {}
    fn flush(&self) {}
    fn has_interactive_ui(&self) -> bool {
        false
    }
    fn start_spinner(&self, _message: &str) -> ActivityToken {
        ActivityToken::default()
    }
    fn start_tool_spinner(&self, _name: &str, _arguments: &serde_json::Value) -> ActivityToken {
        ActivityToken::default()
    }
    fn start_tool_run(&self, _name: &str, _arguments: &serde_json::Value) {}
    fn stream_port(&self) -> ToolStreamPort {
        ToolStreamPort::default()
    }
    fn question_port(&self) -> QuestionPort {
        QuestionPort::new(NoopQuestionPort)
    }
    async fn prompt_tool_approval(&self, _name: &str, _arguments: &serde_json::Value) -> ApprovalResult {
        ApprovalResult::Approved
    }
    async fn prompt_bash_approval(&self, _request: BashApproval<'_>) -> ApprovalResult {
        ApprovalResult::Approved
    }
    async fn prompt_continue_budget(&self, _max_turns: usize) -> bool {
        false
    }
}

pub fn presenter() -> Arc<dyn Presenter> {
    Arc::new(NoopPresenter)
}
