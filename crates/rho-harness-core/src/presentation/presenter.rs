//! The presenter contract: everything the engine emits to the active
//! presentation capability. Layout, styling, and terminal control stay on the
//! presentation side; the engine only passes typed data.

pub use super::types::{
    ApprovalResult, BashApproval, BlockDisplay, InteractionPrompt, InteractionResponse, RiskTier, SessionStatus,
    ToolLine, WelcomeDisplay,
};
use crate::presentation::stream::ToolStreamPort;
use async_trait::async_trait;
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
    async fn prompt_tool_approval(&self, _name: &str, _arguments: &Value) -> ApprovalResult {
        ApprovalResult::Approved
    }
    async fn prompt_bash_approval(&self, _request: BashApproval) -> ApprovalResult {
        ApprovalResult::Approved
    }
    /// Renders a generic modal on behalf of a caller (currently: plugins
    /// serving `ui/prompt`). `None` means no interactive UI — callers treat
    /// that as cancellation.
    async fn request_interaction(&self, _prompt: InteractionPrompt) -> Option<InteractionResponse> {
        None
    }
    async fn prompt_continue_budget(&self, _max_turns: usize) -> bool {
        false
    }
    fn print_turn_started(&self, _prompt: &str) {}
    fn print_turn_completed(&self, _status: &str) {}
    fn print_block(&self, _display: &BlockDisplay) {}
    fn set_extra_status(&self, _status: Option<String>) {}
}
