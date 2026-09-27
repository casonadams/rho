use std::sync::Arc;

use async_trait::async_trait;
use rho_harness_core::collab::{CapabilityLevel, CollabHostServer, CollabIncomingCommand, CollabPeerEvent};
use rho_harness_core::presentation::activity::ActivityToken;
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::stream::ToolStreamPort;
use rho_harness_core::presentation::{
    BlockDisplay, InteractionPrompt, InteractionResponse, SessionStatus, ToolLine, WelcomeDisplay,
};
use rho_harness_core::rpc::protocol::{RpcCommand, RpcEvent};
use serde_json::Value;
use tokio::sync::{Mutex, oneshot};

use super::cancel::cancel_active_turn;
use super::event::TurnInputResources;
use super::runner::TurnLoop;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{QueuedMessage, TerminalBackend};
use crate::ui::render::TerminalRenderer;

pub struct PendingCollabApproval {
    pub approval_id: String,
    pub resolver_tx: oneshot::Sender<InteractionResponse>,
}

#[derive(Clone)]
pub struct CollabPresenter {
    pub(crate) inner: Arc<dyn Presenter>,
    pub(crate) collab: Arc<CollabHostServer>,
    pub(crate) pending_approval: Arc<Mutex<Option<PendingCollabApproval>>>,
}

impl CollabPresenter {
    pub fn new(inner: Arc<dyn Presenter>, collab: Arc<CollabHostServer>) -> Self {
        Self {
            inner,
            collab,
            pending_approval: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn resolve_tool_response(&self, approval_id: &str, decision: &str) -> bool {
        let mut guard = self.pending_approval.lock().await;
        let Some(pending) = guard.as_ref() else {
            return false;
        };
        if pending.approval_id != approval_id {
            return false;
        }
        let Some(pending) = guard.take() else {
            return false;
        };
        let response = match decision {
            "allow" | "yes" | "approve" => InteractionResponse::Selected(0),
            _ => InteractionResponse::Cancelled,
        };
        let _ = pending.resolver_tx.send(response);
        true
    }
}

fn build_tool_approval_event(approval_id: &str, prompt: &InteractionPrompt) -> RpcEvent {
    let arguments = serde_json::json!({
        "body": prompt.body,
        "options": prompt.options,
        "initial_selection": prompt.initial_selection,
        "initial_text": prompt.initial_text,
    });
    RpcEvent::ToolApprovalRequest {
        approval_id: approval_id.to_string(),
        tool: prompt.title.clone(),
        arguments,
        description: Some(prompt.body.clone()),
    }
}

fn decision_from_response(response: Option<&InteractionResponse>) -> &'static str {
    match response {
        Some(InteractionResponse::Selected(0..=2)) => "allow",
        _ => "deny",
    }
}

#[async_trait]
impl Presenter for CollabPresenter {
    fn write_output(&self, text: &str) {
        self.inner.write_output(text);
        if !text.is_empty() {
            self.collab.broadcast(&RpcEvent::TextChunk {
                content: text.to_string(),
            });
        }
    }

    fn print_welcome(&self, display: &WelcomeDisplay) {
        self.inner.print_welcome(display);
    }

    fn print_session_status(&self, display: &SessionStatus) {
        self.inner.print_session_status(display);
    }

    fn print_notice(&self, text: &str) {
        self.inner.print_notice(text);
        if !text.is_empty() {
            self.collab.broadcast(&RpcEvent::TextChunk {
                content: text.to_string(),
            });
        }
    }

    fn print_user_block(&self, input: &str) {
        self.inner.print_user_block(input);
    }

    fn print_token(&self, token: &str) {
        self.inner.print_token(token);
        if !token.is_empty() {
            self.collab.broadcast(&RpcEvent::TextChunk {
                content: token.to_string(),
            });
        }
    }

    fn print_thinking_token(&self, token: &str) {
        self.inner.print_thinking_token(token);
        if !token.is_empty() {
            self.collab.broadcast(&RpcEvent::ReasoningChunk {
                content: token.to_string(),
            });
        }
    }

    fn finish_thinking(&self, thinking_text: &str) {
        self.inner.finish_thinking(thinking_text);
    }

    fn finish_tool_line(&self, line: ToolLine) {
        let call_id = line.name.clone();
        let tool = line.name.clone();
        let output = line.output.clone();
        let is_error = line.is_error;
        let duration_ms = line.duration_ms.unwrap_or(0);
        self.inner.finish_tool_line(line);
        self.collab.broadcast(&RpcEvent::ToolCallResult {
            call_id,
            tool,
            output,
            is_error,
            duration_ms,
        });
    }

    fn flush(&self) {
        self.inner.flush();
    }

    fn has_interactive_ui(&self) -> bool {
        self.inner.has_interactive_ui()
    }

    fn start_spinner(&self, message: &str) -> ActivityToken {
        self.inner.start_spinner(message)
    }

    fn start_tool_spinner(&self, name: &str, arguments: &Value) -> ActivityToken {
        self.collab.broadcast(&RpcEvent::ToolCallStart {
            call_id: uuid::Uuid::new_v4().to_string(),
            tool: name.to_string(),
            arguments: arguments.clone(),
        });
        self.inner.start_tool_spinner(name, arguments)
    }

    fn start_tool_run(&self, name: &str, arguments: &Value) {
        self.inner.start_tool_run(name, arguments);
        self.collab.broadcast(&RpcEvent::ToolCallStart {
            call_id: uuid::Uuid::new_v4().to_string(),
            tool: name.to_string(),
            arguments: arguments.clone(),
        });
    }

    fn stream_port(&self) -> ToolStreamPort {
        self.inner.stream_port()
    }

    async fn request_interaction(&self, prompt: InteractionPrompt) -> Option<InteractionResponse> {
        let approval_id = format!("appr-{}", uuid::Uuid::new_v4());
        let req_event = build_tool_approval_event(&approval_id, &prompt);
        self.collab.broadcast(&req_event);

        let (guest_tx, guest_rx) = oneshot::channel();
        {
            let mut guard = self.pending_approval.lock().await;
            *guard = Some(PendingCollabApproval {
                approval_id: approval_id.clone(),
                resolver_tx: guest_tx,
            });
        }

        let host_fut = self.inner.request_interaction(prompt);
        let (decision_str, response) = tokio::select! {
            host_resp = host_fut => {
                (decision_from_response(host_resp.as_ref()), host_resp)
            }
            guest_resp = guest_rx => {
                self.inner.dismiss_interaction();
                let resp = guest_resp.ok().unwrap_or(InteractionResponse::Cancelled);
                (decision_from_response(Some(&resp)), Some(resp))
            }
        };

        {
            let mut guard = self.pending_approval.lock().await;
            *guard = None;
        }

        self.collab.broadcast(&RpcEvent::ToolApprovalResolved {
            approval_id,
            decision: Some(decision_str.to_string()),
        });

        response
    }

    fn dismiss_interaction(&self) {
        self.inner.dismiss_interaction();
    }

    async fn prompt_continue_budget(&self, max_turns: usize) -> bool {
        self.inner.prompt_continue_budget(max_turns).await
    }

    fn print_turn_started(&self, prompt: &str) {
        self.inner.print_turn_started(prompt);
        self.collab.broadcast(&RpcEvent::TurnStart {
            turn_number: 1,
            prompt: prompt.to_string(),
        });
    }

    fn print_turn_completed(&self, status: &str) {
        self.inner.print_turn_completed(status);
        self.collab.broadcast(&RpcEvent::TurnEnd {
            stop_reason: status.to_string(),
        });
    }

    fn print_block(&self, display: &BlockDisplay) {
        self.inner.print_block(display);
    }

    fn set_extra_status(&self, status: Option<String>) {
        self.inner.set_extra_status(status);
    }
}

async fn handle_collab_abort<B: TerminalBackend>(
    lp: &mut TurnLoop<'_, B>,
    res: &mut TurnInputResources<'_>,
) -> Result<bool> {
    cancel_active_turn(lp, res.ui_events, res.cancellation).await?;
    Ok(true)
}

fn handle_collab_steer<B: TerminalBackend>(lp: &mut TurnLoop<'_, B>, message: String) -> bool {
    lp.steering.enqueue(message.clone());
    lp.session
        .renderer
        .print_notice(&format!("\n● Collaborator steering: {message}\n"));
    false
}

async fn handle_collab_tool_response(
    collab_presenter: Option<&CollabPresenter>,
    approval_id: &str,
    decision: &str,
) -> bool {
    if let Some(cp) = collab_presenter {
        let _ = cp.resolve_tool_response(approval_id, decision).await;
    }
    false
}

async fn handle_collab_busy_prompt(collab: Option<&Arc<CollabHostServer>>, peer_id: usize) -> bool {
    if let Some(collab) = collab {
        let err_event = RpcEvent::Error {
            code: "BUSY".into(),
            message: "Turn already in progress on host".into(),
        };
        let _ = collab.send_to_peer(peer_id, &err_event).await;
    }
    false
}

async fn dispatch_non_abort_collab_command<B: TerminalBackend>(
    lp: &mut TurnLoop<'_, B>,
    collab_presenter: Option<&CollabPresenter>,
    incoming: CollabIncomingCommand,
) {
    match incoming.command {
        RpcCommand::Steer { message } => {
            handle_collab_steer(lp, message);
        }
        RpcCommand::ToolResponse { approval_id, decision } => {
            handle_collab_tool_response(collab_presenter, &approval_id, &decision).await;
        }
        RpcCommand::Prompt { .. } => {
            handle_collab_busy_prompt(lp.session.collab.as_ref(), incoming.peer_id).await;
        }
        _ => {}
    }
}

pub(super) async fn handle_collab_turn_command<B: TerminalBackend>(
    lp: &mut TurnLoop<'_, B>,
    res: &mut TurnInputResources<'_>,
    collab_presenter: Option<&CollabPresenter>,
    incoming: CollabIncomingCommand,
) -> Result<bool> {
    if matches!(incoming.command, RpcCommand::Abort) {
        return handle_collab_abort(lp, res).await;
    }
    dispatch_non_abort_collab_command(lp, collab_presenter, incoming).await;
    Ok(false)
}

pub fn handle_collab_idle_command(incoming: CollabIncomingCommand, session: &ReplSession) -> Option<QueuedMessage> {
    match incoming.command {
        RpcCommand::Prompt { message, .. } => {
            session
                .renderer
                .print_notice(&format!("● Collaborator prompt: {message}\n"));
            Some(QueuedMessage {
                text: message,
                kind: crate::ui::interactive::QueueKind::FollowUp,
            })
        }
        _ => None,
    }
}

pub fn handle_collab_peer_event(event: &CollabPeerEvent, renderer: &TerminalRenderer, peer_count: usize) {
    match event {
        CollabPeerEvent::Connected(info) => {
            let role_label = match info.role {
                CapabilityLevel::Full => "co-pilot",
                CapabilityLevel::ViewOnly => "spectator",
            };
            renderer.print_notice(&format!(
                "\n  ● Collaborator connected: {} ({role_label}, {peer_count} peer(s) total)\n",
                info.display_name()
            ));
        }
        CollabPeerEvent::Disconnected { display_name, .. } => {
            renderer.print_notice(&format!(
                "\n  ● Collaborator {display_name} disconnected ({peer_count} peer(s) remaining)\n"
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_harness_core::collab::CollabHostConfig;
    use std::net::SocketAddr;

    #[tokio::test]
    async fn test_collab_presenter_resolve_tool_response() {
        let config = CollabHostConfig::new().with_bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)));
        let server = Arc::new(CollabHostServer::start(config).await.expect("start server"));
        let (renderer, _) = crate::ui::interactive::InteractiveUi::channel();
        let term_renderer = Arc::new(TerminalRenderer::with_ui(renderer));
        let presenter = CollabPresenter::new(term_renderer, server);

        let (tx, rx) = oneshot::channel();
        {
            let mut guard = presenter.pending_approval.lock().await;
            *guard = Some(PendingCollabApproval {
                approval_id: "appr-123".into(),
                resolver_tx: tx,
            });
        }

        assert!(!presenter.resolve_tool_response("wrong-id", "allow").await);
        assert!(presenter.resolve_tool_response("appr-123", "allow").await);
        let res = rx.await.unwrap();
        assert_eq!(res, InteractionResponse::Selected(0));
    }

    #[tokio::test]
    async fn test_collab_presenter_resolve_deny_tool_response() {
        let config = CollabHostConfig::new().with_bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)));
        let server = Arc::new(CollabHostServer::start(config).await.expect("start server"));
        let (renderer, _) = crate::ui::interactive::InteractiveUi::channel();
        let term_renderer = Arc::new(TerminalRenderer::with_ui(renderer));
        let presenter = CollabPresenter::new(term_renderer, server);

        let (tx, rx) = oneshot::channel();
        {
            let mut guard = presenter.pending_approval.lock().await;
            *guard = Some(PendingCollabApproval {
                approval_id: "appr-456".into(),
                resolver_tx: tx,
            });
        }

        assert!(presenter.resolve_tool_response("appr-456", "deny").await);
        let res = rx.await.unwrap();
        assert_eq!(res, InteractionResponse::Cancelled);
    }

    #[test]
    fn test_handle_collab_idle_command_prompt() {
        let session = ReplSession::new(
            rho_harness_core::config::Config::default(),
            rho_engine::auth::AuthStore::default(),
            None,
        );
        let incoming = CollabIncomingCommand {
            peer_id: 1,
            role: CapabilityLevel::Full,
            command: RpcCommand::Prompt {
                message: "write hello world".into(),
                images: None,
                streaming_behavior: None,
            },
        };
        let msg = handle_collab_idle_command(incoming, &session);
        assert!(matches!(msg, Some(QueuedMessage { text, .. }) if text == "write hello world"));
    }

    #[test]
    fn test_handle_collab_peer_event() {
        let (ui, _) = crate::ui::interactive::InteractiveUi::channel();
        let renderer = TerminalRenderer::with_ui(ui);
        let event_connected = CollabPeerEvent::Connected(rho_harness_core::collab::CollabPeerInfo {
            id: 1,
            role: CapabilityLevel::Full,
            hostname: Some("worker-1".into()),
            connected_at: chrono::Utc::now(),
        });
        handle_collab_peer_event(&event_connected, &renderer, 1);

        let event_disconnected = CollabPeerEvent::Disconnected {
            peer_id: 1,
            display_name: "worker-1".into(),
            reason: "Stream closed".into(),
        };
        handle_collab_peer_event(&event_disconnected, &renderer, 0);
    }

    #[tokio::test]
    async fn test_handle_collab_tool_response_and_busy() {
        let config = CollabHostConfig::new().with_bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)));
        let server = Arc::new(CollabHostServer::start(config).await.expect("start server"));
        let (renderer, _) = crate::ui::interactive::InteractiveUi::channel();
        let term_renderer = Arc::new(TerminalRenderer::with_ui(renderer));
        let presenter = CollabPresenter::new(term_renderer, Arc::clone(&server));

        let res = handle_collab_tool_response(Some(&presenter), "non-existent", "allow").await;
        assert!(!res);

        let busy = handle_collab_busy_prompt(Some(&server), 99).await;
        assert!(!busy);

        let none_busy = handle_collab_busy_prompt(None, 99).await;
        assert!(!none_busy);
    }
}
