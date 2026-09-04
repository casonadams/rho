use std::sync::Mutex;

use async_trait::async_trait;
use rho_harness_core::presentation::activity::ActivityToken;
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::stream::ToolStreamPort;
use rho_harness_core::presentation::{SessionStatus, ToolLine, WelcomeDisplay};

#[derive(Default)]
pub struct CapturingPresenter {
    pub notices: Mutex<Vec<String>>,
}

#[async_trait]
impl Presenter for CapturingPresenter {
    fn write_output(&self, _text: &str) {}
    fn print_welcome(&self, _display: &WelcomeDisplay) {}
    fn print_session_status(&self, _display: &SessionStatus) {}
    fn print_notice(&self, text: &str) {
        self.notices.lock().unwrap().push(text.to_string());
    }
    fn print_user_block(&self, _input: &str) {}
    fn print_token(&self, _token: &str) {}
    fn print_thinking_token(&self, _token: &str) {}
    fn finish_tool_line(&self, _line: ToolLine) {}
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
    async fn prompt_continue_budget(&self, _max_turns: usize) -> bool {
        false
    }
}
