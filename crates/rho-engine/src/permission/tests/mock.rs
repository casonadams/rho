use async_trait::async_trait;
use rho_harness_core::presentation::activity::ActivityToken;
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::stream::ToolStreamPort;
use rho_harness_core::presentation::{
    BlockDisplay, InteractionPrompt, InteractionResponse, SessionStatus, ToolLine, WelcomeDisplay,
};
use std::sync::Mutex;

pub struct MockHookPresenter {
    pub has_ui: bool,
    pub response: Mutex<Option<InteractionResponse>>,
    pub last_prompt: Mutex<Option<InteractionPrompt>>,
}

impl MockHookPresenter {
    pub fn new(has_ui: bool, response: Option<InteractionResponse>) -> Self {
        Self {
            has_ui,
            response: Mutex::new(response),
            last_prompt: Mutex::new(None),
        }
    }
}

#[async_trait]
impl Presenter for MockHookPresenter {
    fn write_output(&self, _text: &str) {}
    fn print_welcome(&self, _display: &WelcomeDisplay) {}
    fn print_session_status(&self, _display: &SessionStatus) {}
    fn print_notice(&self, _text: &str) {}
    fn print_block(&self, _display: &BlockDisplay) {}
    fn set_extra_status(&self, _status: Option<String>) {}
    fn print_user_block(&self, _input: &str) {}
    fn print_token(&self, _token: &str) {}
    fn print_thinking_token(&self, _token: &str) {}
    fn finish_tool_line(&self, _line: ToolLine) {}
    fn flush(&self) {}
    fn has_interactive_ui(&self) -> bool {
        self.has_ui
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
    async fn request_interaction(&self, prompt: InteractionPrompt) -> Option<InteractionResponse> {
        if let Ok(mut slot) = self.last_prompt.lock() {
            *slot = Some(prompt);
        }
        self.response.lock().ok().and_then(|r| r.clone())
    }
}
