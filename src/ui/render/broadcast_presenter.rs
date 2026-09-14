use crate::platform::remote::PeerRegistry;
use async_trait::async_trait;
use rho_harness_core::presentation::activity::ActivityToken;
use rho_harness_core::presentation::stream::ToolStreamPort;
use rho_harness_core::presentation::{
    BlockDisplay, InteractionPrompt, InteractionResponse, Presenter, SessionStatus, ToolLine, WelcomeDisplay,
};
use rho_harness_core::rpc::protocol::RpcEvent;
use serde_json::Value;
use std::sync::Arc;

pub struct BroadcastPresenter {
    local: Arc<dyn Presenter>,
    peers: PeerRegistry,
}

impl BroadcastPresenter {
    pub fn new(local: Arc<dyn Presenter>, peers: PeerRegistry) -> Self {
        Self { local, peers }
    }
}

#[async_trait]
impl Presenter for BroadcastPresenter {
    fn write_output(&self, text: &str) {
        self.local.write_output(text);
        if !text.is_empty() {
            self.peers.broadcast(&RpcEvent::TextChunk {
                content: text.to_string(),
            });
        }
    }

    fn print_welcome(&self, display: &WelcomeDisplay) {
        self.local.print_welcome(display);
    }

    fn print_session_status(&self, display: &SessionStatus) {
        self.local.print_session_status(display);
        self.peers.broadcast(&RpcEvent::SessionStart {
            session_id: String::new(),
            model: display.model.clone(),
            provider: display.provider.clone(),
        });
    }

    fn print_notice(&self, text: &str) {
        self.local.print_notice(text);
    }

    fn print_user_block(&self, input: &str) {
        self.local.print_user_block(input);
    }

    fn print_token(&self, token: &str) {
        self.local.print_token(token);
        if !token.is_empty() {
            self.peers.broadcast(&RpcEvent::TextChunk {
                content: token.to_string(),
            });
        }
    }

    fn print_thinking_token(&self, token: &str) {
        self.local.print_thinking_token(token);
        if !token.is_empty() {
            self.peers.broadcast(&RpcEvent::ReasoningChunk {
                content: token.to_string(),
            });
        }
    }

    fn finish_thinking(&self, thinking_text: &str) {
        self.local.finish_thinking(thinking_text);
    }

    fn finish_tool_line(&self, line: ToolLine) {
        self.peers.broadcast(&RpcEvent::ToolCallResult {
            call_id: line.name.clone(),
            tool: line.name.clone(),
            output: line.output.clone(),
            is_error: line.is_error,
            duration_ms: line.duration_ms.unwrap_or(0),
        });
        self.local.finish_tool_line(line);
    }

    fn flush(&self) {
        self.local.flush();
    }

    fn has_interactive_ui(&self) -> bool {
        self.local.has_interactive_ui()
    }

    fn start_spinner(&self, message: &str) -> ActivityToken {
        self.local.start_spinner(message)
    }

    fn start_tool_spinner(&self, name: &str, arguments: &Value) -> ActivityToken {
        self.local.start_tool_spinner(name, arguments)
    }

    fn start_tool_run(&self, name: &str, arguments: &Value) {
        self.peers.broadcast(&RpcEvent::ToolCallStart {
            call_id: name.to_string(),
            tool: name.to_string(),
            arguments: arguments.clone(),
        });
        self.local.start_tool_run(name, arguments);
    }

    fn stream_port(&self) -> ToolStreamPort {
        self.local.stream_port()
    }

    async fn request_interaction(&self, prompt: InteractionPrompt) -> Option<InteractionResponse> {
        self.local.request_interaction(prompt).await
    }

    async fn prompt_continue_budget(&self, max_turns: usize) -> bool {
        self.local.prompt_continue_budget(max_turns).await
    }

    fn print_turn_started(&self, prompt: &str) {
        self.peers.broadcast(&RpcEvent::TurnStart {
            turn_number: 1,
            prompt: prompt.to_string(),
        });
        self.peers.broadcast(&RpcEvent::StatusChanged {
            status: "busy".to_string(),
        });
        self.local.print_turn_started(prompt);
    }

    fn print_turn_completed(&self, status: &str) {
        self.peers.broadcast(&RpcEvent::TurnEnd {
            stop_reason: status.to_string(),
        });
        self.peers.broadcast(&RpcEvent::StatusChanged {
            status: "idle".to_string(),
        });
        self.local.print_turn_completed(status);
    }

    fn print_block(&self, display: &BlockDisplay) {
        self.local.print_block(display);
    }

    fn set_extra_status(&self, status: Option<String>) {
        if let Some(ref s) = status {
            self.peers.broadcast(&RpcEvent::StatusChanged { status: s.clone() });
        }
        self.local.set_extra_status(status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    struct DummyPresenter;
    #[async_trait]
    impl Presenter for DummyPresenter {
        fn write_output(&self, _text: &str) {}
        fn print_welcome(&self, _display: &WelcomeDisplay) {}
        fn print_session_status(&self, _display: &SessionStatus) {}
        fn print_notice(&self, _text: &str) {}
        fn print_user_block(&self, _input: &str) {}
        fn print_token(&self, _token: &str) {}
        fn print_thinking_token(&self, _token: &str) {}
        fn finish_tool_line(&self, _line: ToolLine) {}
        fn flush(&self) {}
        fn has_interactive_ui(&self) -> bool {
            true
        }
        fn start_spinner(&self, _message: &str) -> ActivityToken {
            ActivityToken::default()
        }
        fn start_tool_spinner(&self, _name: &str, _arguments: &Value) -> ActivityToken {
            ActivityToken::default()
        }
        fn start_tool_run(&self, _name: &str, _arguments: &Value) {}
        fn stream_port(&self) -> ToolStreamPort {
            ToolStreamPort::default()
        }
    }

    #[tokio::test]
    async fn test_broadcast_presenter_fans_out() {
        let registry = PeerRegistry::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        registry.register(tx);

        let presenter = BroadcastPresenter::new(Arc::new(DummyPresenter), registry);
        presenter.print_token("Hello live world!");

        let ev = rx.recv().await.unwrap();
        match ev {
            RpcEvent::TextChunk { content } => assert_eq!(content, "Hello live world!"),
            other => panic!("expected TextChunk, got {other:?}"),
        }
    }
}
