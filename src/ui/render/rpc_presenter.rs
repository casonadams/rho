use async_trait::async_trait;
use rho_core::presentation::activity::ActivityToken;
use rho_core::presentation::presenter::Presenter;
use rho_core::presentation::questions::QuestionPort;
use rho_core::presentation::stream::ToolStreamPort;
use rho_core::rpc::protocol::RpcEvent;
use rho_sdk::ui::{ApprovalResult, BashApproval, SessionStatus, ToolLine, WelcomeDisplay};
use serde_json::Value;
use tokio::sync::mpsc;

struct RpcQuestionPort;

#[async_trait]
impl rho_core::presentation::questions::InteractiveQuestionPort for RpcQuestionPort {
    async fn ask(
        &self,
        _question: rho_core::presentation::questions::UserQuestion,
    ) -> rho_core::error::Result<rho_core::presentation::questions::UserAnswer> {
        Err(rho_core::error::AppError::Session(
            "interactive questions over RPC should use structured parameters".to_string(),
        ))
    }
}

#[derive(Clone)]
pub struct RpcPresenter {
    event_tx: mpsc::UnboundedSender<RpcEvent>,
}

impl RpcPresenter {
    pub fn new(event_tx: mpsc::UnboundedSender<RpcEvent>) -> (Self, mpsc::UnboundedSender<(String, bool)>) {
        let (approval_tx, _approval_rx) = mpsc::unbounded_channel();
        let presenter = Self { event_tx };
        (presenter, approval_tx)
    }

    pub fn emit(&self, event: RpcEvent) {
        let _ = self.event_tx.send(event);
    }
}

#[async_trait]
impl Presenter for RpcPresenter {
    fn write_output(&self, text: &str) {
        if !text.is_empty() {
            self.emit(RpcEvent::TextChunk {
                content: text.to_string(),
            });
        }
    }

    fn print_welcome(&self, _display: &WelcomeDisplay) {}

    fn print_session_status(&self, display: &SessionStatus) {
        self.emit(RpcEvent::SessionStart {
            session_id: String::new(),
            model: display.model.clone(),
            provider: display.provider.clone(),
        });
    }

    fn print_notice(&self, text: &str) {
        if !text.is_empty() {
            self.emit(RpcEvent::TextChunk {
                content: text.to_string(),
            });
        }
    }

    fn print_user_block(&self, _input: &str) {}

    fn print_token(&self, token: &str) {
        self.emit(RpcEvent::TextChunk {
            content: token.to_string(),
        });
    }

    fn print_thinking_token(&self, token: &str) {
        self.emit(RpcEvent::ReasoningChunk {
            content: token.to_string(),
        });
    }

    fn finish_tool_line(&self, line: ToolLine) {
        self.emit(RpcEvent::ToolCallResult {
            call_id: line.name.clone(),
            tool: line.name,
            output: line.output,
            is_error: line.is_error,
            duration_ms: line.duration_ms.unwrap_or(0),
        });
    }

    fn flush(&self) {}

    fn has_interactive_ui(&self) -> bool {
        false
    }

    fn start_spinner(&self, _message: &str) -> ActivityToken {
        ActivityToken::default()
    }

    fn start_tool_spinner(&self, name: &str, arguments: &Value) -> ActivityToken {
        self.start_tool_run(name, arguments);
        ActivityToken::default()
    }

    fn start_tool_run(&self, name: &str, arguments: &Value) {
        self.emit(RpcEvent::ToolCallStart {
            call_id: uuid::Uuid::new_v4().to_string(),
            tool: name.to_string(),
            arguments: arguments.clone(),
        });
    }

    fn stream_port(&self) -> ToolStreamPort {
        ToolStreamPort::default()
    }

    fn question_port(&self) -> QuestionPort {
        QuestionPort::new(RpcQuestionPort)
    }

    async fn prompt_tool_approval(&self, name: &str, arguments: &Value) -> ApprovalResult {
        let approval_id = uuid::Uuid::new_v4().to_string();
        self.emit(RpcEvent::ToolApprovalRequest {
            approval_id: approval_id.clone(),
            tool: name.to_string(),
            arguments: arguments.clone(),
            description: None,
        });

        ApprovalResult::Approved
    }

    async fn prompt_bash_approval(&self, request: BashApproval) -> ApprovalResult {
        let approval_id = uuid::Uuid::new_v4().to_string();
        self.emit(RpcEvent::ToolApprovalRequest {
            approval_id: approval_id.clone(),
            tool: "bash".to_string(),
            arguments: serde_json::json!({ "command": request.command }),
            description: request.reasons.first().cloned(),
        });

        ApprovalResult::Approved
    }

    async fn prompt_continue_budget(&self, _max_turns: usize) -> bool {
        true
    }

    fn print_turn_started(&self, prompt: &str) {
        self.emit(RpcEvent::TurnStart {
            turn_number: 1,
            prompt: prompt.to_string(),
        });
    }

    fn print_turn_completed(&self, status: &str) {
        self.emit(RpcEvent::TurnEnd {
            stop_reason: status.to_string(),
        });
    }
}
