use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use rho_harness_core::presentation::activity::ActivityToken;
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::stream::ToolStreamPort;
use rho_harness_core::presentation::{InteractionPrompt, InteractionResponse, SessionStatus, ToolLine, WelcomeDisplay};
use rho_harness_core::rpc::protocol::RpcEvent;
use serde_json::Value;
use tokio::sync::{Mutex, mpsc, oneshot};

pub type PendingApprovals = Arc<Mutex<HashMap<String, oneshot::Sender<InteractionResponse>>>>;

#[derive(Clone)]
pub struct RpcPresenter {
    event_tx: mpsc::UnboundedSender<RpcEvent>,
    pending_approvals: PendingApprovals,
}

impl RpcPresenter {
    pub fn new(event_tx: mpsc::UnboundedSender<RpcEvent>) -> Self {
        Self {
            event_tx,
            pending_approvals: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_approvals(event_tx: mpsc::UnboundedSender<RpcEvent>, pending_approvals: PendingApprovals) -> Self {
        Self {
            event_tx,
            pending_approvals,
        }
    }

    pub fn pending_approvals(&self) -> PendingApprovals {
        Arc::clone(&self.pending_approvals)
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
        true
    }

    async fn request_interaction(&self, prompt: InteractionPrompt) -> Option<InteractionResponse> {
        let approval_id = format!("appr-{}", uuid::Uuid::new_v4());
        let (tx, rx) = oneshot::channel();
        self.pending_approvals.lock().await.insert(approval_id.clone(), tx);

        let arguments = serde_json::json!({
            "body": prompt.body,
            "options": prompt.options,
            "initial_selection": prompt.initial_selection,
            "initial_text": prompt.initial_text,
        });

        self.emit(RpcEvent::ToolApprovalRequest {
            approval_id: approval_id.clone(),
            tool: prompt.title,
            arguments,
            description: Some(prompt.body),
        });
        self.emit(RpcEvent::StatusChanged {
            status: "waiting_approval".to_string(),
        });

        let response = match rx.await {
            Ok(res) => Some(res),
            Err(_) => Some(InteractionResponse::Cancelled),
        };
        self.emit(RpcEvent::StatusChanged {
            status: "busy".to_string(),
        });
        response
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
