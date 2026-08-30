//! The presenter contract: everything the engine emits to the active
//! presentation capability. Layout, styling, and terminal control stay on the
//! presentation side; the engine only passes typed data.

use crate::presentation::questions::QuestionPort;
use crate::presentation::stream::ToolStreamPort;
use async_trait::async_trait;
pub use rho_sdk::ui::{ApprovalResult, BashApproval, RiskTier, SessionStatus, ToolLine, WelcomeDisplay};
use serde_json::Value;

use super::activity::ActivityToken;

#[async_trait]
pub trait Presenter: Send + Sync {
    fn write_output(&self, text: &str);
    fn print_welcome(&self, display: &WelcomeDisplay);
    fn print_session_status(&self, display: &SessionStatus);
    fn print_notice(&self, text: &str);
    fn print_user_block(&self, input: &str);
    fn print_token(&self, token: &str);
    fn print_thinking_token(&self, token: &str);
    fn finish_tool_line(&self, line: ToolLine);
    fn flush(&self);
    fn has_interactive_ui(&self) -> bool;
    fn start_spinner(&self, message: &str) -> ActivityToken;
    fn start_tool_spinner(&self, name: &str, arguments: &Value) -> ActivityToken;
    fn start_tool_run(&self, name: &str, arguments: &Value);
    fn stream_port(&self) -> ToolStreamPort;
    fn question_port(&self) -> QuestionPort;
    async fn prompt_tool_approval(&self, name: &str, arguments: &Value) -> ApprovalResult;
    async fn prompt_bash_approval(&self, request: BashApproval) -> ApprovalResult;
    async fn prompt_continue_budget(&self, max_turns: usize) -> bool;
}
