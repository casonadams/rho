use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use iroh::Endpoint;
use iroh::endpoint::presets::N0;
use rho_harness_core::collab::protocol::COLLAB_ALPN;
use rho_harness_core::collab::session::{CollabReader, CollabSessionStream, CollabWriter};
use rho_harness_core::collab::{CapabilityLevel, CollabSnapshot, CollabTicket};
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::rpc::protocol::{RpcCommand, RpcEvent};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::repl::input_reader::TerminalInputReader;
use crate::ui::TerminalRenderer;

struct RawModeGuard;

impl RawModeGuard {
    fn enter() -> Result<Self> {
        let _ = crossterm::terminal::enable_raw_mode();
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

pub(crate) fn print_connection_banner(
    renderer: &TerminalRenderer,
    role: CapabilityLevel,
    snapshot: Option<&CollabSnapshot>,
) {
    let role_label = match role {
        CapabilityLevel::Full => "co-pilot",
        CapabilityLevel::ViewOnly => "spectator",
    };
    renderer.print_notice(&format!("● Connected to host session ({role_label})\n"));

    if let Some(snap) = snapshot
        && !snap.turns.is_empty()
    {
        renderer.print_notice(&format!("  Restored {} prior turn(s)\n", snap.turns.len()));
    }

    if role == CapabilityLevel::ViewOnly {
        renderer.print_notice("  [View Only - Read Mode]\n\n");
    }
}

pub async fn run_guest_client(ticket_str: &str) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let clean_ticket = ticket_str.trim().trim_start_matches("rho join ").trim();
    let ticket = CollabTicket::parse(clean_ticket)?;

    let endpoint = Endpoint::builder(N0)
        .bind()
        .await
        .map_err(|e| AppError::Network(format!("Failed to bind client endpoint: {e}")))?;

    let renderer = TerminalRenderer::default();
    let (writer, reader, snapshot) = connect_and_handshake(&endpoint, &ticket).await?;

    print_connection_banner(&renderer, reader.role(), snapshot.as_ref());

    let input = TerminalInputReader::spawn()?;
    let _raw_guard = RawModeGuard::enter()?;
    run_guest_session(writer, reader, &renderer, input).await?;

    Ok(())
}

pub async fn connect_and_handshake(
    endpoint: &Endpoint,
    ticket: &CollabTicket,
) -> Result<(
    CollabWriter<iroh::endpoint::SendStream>,
    CollabReader<iroh::endpoint::RecvStream>,
    Option<CollabSnapshot>,
)> {
    let addr = ticket.to_endpoint_addr();
    let conn = endpoint
        .connect(addr, COLLAB_ALPN)
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;
    let (send, recv) = conn.accept_bi().await.map_err(|e| AppError::Network(e.to_string()))?;
    let mut stream = CollabSessionStream::connect_guest_with_ticket(send, recv, ticket).await?;
    let snapshot = stream.recv_snapshot().await?;
    let (writer, reader) = stream.into_split();
    Ok((writer, reader, snapshot))
}

pub(crate) async fn run_guest_session<W: AsyncWrite + Unpin, R: AsyncRead + Unpin>(
    mut writer: CollabWriter<W>,
    mut reader: CollabReader<R>,
    renderer: &TerminalRenderer,
    mut input: TerminalInputReader,
) -> Result<()> {
    let role = reader.role();
    let mut prompt_buf = String::new();
    let mut pending_approval: Option<String> = None;

    loop {
        tokio::select! {
            event_res = reader.recv_event() => {
                match event_res {
                    Ok(Some(event)) => {
                        if !handle_incoming_rpc_event(&event, renderer, &mut pending_approval) {
                            break;
                        }
                    }
                    Ok(None) => {
                        renderer.print_notice("\n● Host disconnected.\n");
                        break;
                    }
                    Err(e) => {
                        renderer.print_notice(&format!("\n● Connection error: {e}\n"));
                        break;
                    }
                }
            }
            input_opt = input.recv() => {
                let Some(input_res) = input_opt else {
                    break;
                };
                let Ok(event) = input_res else {
                    continue;
                };
                if let Event::Key(key) = event
                    && key.kind == KeyEventKind::Press
                    && !handle_guest_key_event(
                        key,
                        role,
                        &mut prompt_buf,
                        &mut pending_approval,
                        &mut writer,
                        renderer,
                    ).await?
                {
                    break;
                }
            }
        }
    }

    Ok(())
}

pub(crate) fn handle_incoming_rpc_event(
    event: &RpcEvent,
    renderer: &TerminalRenderer,
    pending_approval: &mut Option<String>,
) -> bool {
    match event {
        RpcEvent::TextChunk { content } => {
            renderer.print_token(content);
            renderer.flush();
        }
        RpcEvent::ReasoningChunk { content } => {
            renderer.print_thinking_token(content);
            renderer.flush();
        }
        RpcEvent::TurnStart { prompt, .. } => {
            renderer.print_user_block(prompt);
            renderer.flush();
        }
        RpcEvent::TurnEnd { .. } => {
            renderer.write_output("\n");
            renderer.flush();
        }
        RpcEvent::ToolCallStart { tool, arguments, .. } => {
            renderer.start_tool_run(tool, arguments);
            renderer.flush();
        }
        RpcEvent::ToolCallResult {
            tool, is_error, output, ..
        } => {
            renderer.write_output(&format!("\n[{tool} (error: {is_error})]\n{output}\n"));
            renderer.flush();
        }
        RpcEvent::ToolApprovalRequest { approval_id, tool, .. } => {
            *pending_approval = Some(approval_id.clone());
            renderer.print_notice(&format!(
                "\n? Approval required for tool '{tool}': [y] approve, [n] reject\n"
            ));
            renderer.flush();
        }
        RpcEvent::ToolApprovalResolved { approval_id, decision } => {
            *pending_approval = None;
            let dec = decision.as_deref().unwrap_or("resolved");
            renderer.print_notice(&format!("\n✓ Tool approval {approval_id} resolved: {dec}\n"));
            renderer.flush();
        }
        RpcEvent::Error { code, message } => {
            return handle_rpc_error_event(code, message, renderer);
        }
        RpcEvent::StatusChanged { status } => {
            renderer.print_notice(&format!("── {status}\n"));
            renderer.flush();
        }
        _ => {}
    }
    true
}

fn handle_rpc_error_event(code: &str, message: &str, renderer: &TerminalRenderer) -> bool {
    if code == "KICKED" {
        renderer.print_notice("\n● You were disconnected by the host.\n");
        return false;
    }
    if code == "ROTATED" {
        renderer.print_notice("\n● Host rotated session keys. Session ended.\n");
        return false;
    }
    if code == "STOPPED" {
        renderer.print_notice("\n● Collab session stopped by host.\n");
        return false;
    }
    if code == "UNAUTHORIZED" {
        renderer.print_notice(&format!("\n[Unauthorized: {message}]\n"));
        return true;
    }
    renderer.print_notice(&format!("\nError [{code}]: {message}\n"));
    renderer.flush();
    true
}

async fn handle_approval_key<W: AsyncWrite + Unpin>(
    key: KeyEvent,
    pending_approval: &mut Option<String>,
    writer: &mut CollabWriter<W>,
    renderer: &TerminalRenderer,
) -> Result<bool> {
    let Some(approval_id) = pending_approval.take() else {
        return Ok(false);
    };
    match key.code {
        KeyCode::Char('y' | 'Y') => {
            renderer.print_notice("\n✓ Approved tool execution\n");
            writer
                .send_command(&RpcCommand::ToolResponse {
                    approval_id,
                    decision: "allow".into(),
                })
                .await?;
            Ok(true)
        }
        KeyCode::Char('n' | 'N') => {
            renderer.print_notice("\n✗ Rejected tool execution\n");
            writer
                .send_command(&RpcCommand::ToolResponse {
                    approval_id,
                    decision: "deny".into(),
                })
                .await?;
            Ok(true)
        }
        _ => {
            *pending_approval = Some(approval_id);
            Ok(false)
        }
    }
}

fn handle_control_keys(key: KeyEvent, prompt_buf: &mut String, renderer: &TerminalRenderer) -> Option<bool> {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if prompt_buf.is_empty() {
            return Some(false);
        }
        prompt_buf.clear();
        renderer.print_notice("\n^C\n");
        return Some(true);
    }

    if key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL) && prompt_buf.is_empty() {
        return Some(false);
    }
    None
}

pub(crate) async fn handle_guest_key_event<W: AsyncWrite + Unpin>(
    key: KeyEvent,
    role: CapabilityLevel,
    prompt_buf: &mut String,
    pending_approval: &mut Option<String>,
    writer: &mut CollabWriter<W>,
    renderer: &TerminalRenderer,
) -> Result<bool> {
    if let Some(res) = handle_control_keys(key, prompt_buf, renderer) {
        return Ok(res);
    }

    if role == CapabilityLevel::ViewOnly {
        if key.code == KeyCode::Char('q') {
            return Ok(false);
        }
        renderer.print_notice("[View Only - Read Mode]\n");
        return Ok(true);
    }

    if handle_approval_key(key, pending_approval, writer, renderer).await? {
        return Ok(true);
    }

    match key.code {
        KeyCode::Enter => {
            let trimmed = prompt_buf.trim().to_string();
            prompt_buf.clear();
            if !trimmed.is_empty() {
                renderer.print_notice(&format!("> {trimmed}\n"));
                writer
                    .send_command(&RpcCommand::Prompt {
                        message: trimmed,
                        images: None,
                        streaming_behavior: None,
                    })
                    .await?;
            }
        }
        KeyCode::Esc => {
            writer.send_command(&RpcCommand::Abort).await?;
        }
        KeyCode::Backspace => {
            prompt_buf.pop();
        }
        KeyCode::Char(c) => {
            prompt_buf.push(c);
        }
        _ => {}
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    use tokio::io::duplex;

    fn make_key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }
    }

    #[tokio::test]
    async fn test_handle_guest_key_event_view_only_blocks_input() {
        let (send, _recv) = duplex(1024);
        let mut writer = CollabWriter::new(send, CapabilityLevel::ViewOnly);
        let renderer = TerminalRenderer::default();
        let mut prompt_buf = String::new();
        let mut pending_approval = None;

        let key_a = make_key(KeyCode::Char('a'), KeyModifiers::empty());
        let res = handle_guest_key_event(
            key_a,
            CapabilityLevel::ViewOnly,
            &mut prompt_buf,
            &mut pending_approval,
            &mut writer,
            &renderer,
        )
        .await
        .unwrap();
        assert!(res);
        assert!(prompt_buf.is_empty());

        let key_q = make_key(KeyCode::Char('q'), KeyModifiers::empty());
        let res_q = handle_guest_key_event(
            key_q,
            CapabilityLevel::ViewOnly,
            &mut prompt_buf,
            &mut pending_approval,
            &mut writer,
            &renderer,
        )
        .await
        .unwrap();
        assert!(!res_q);
    }

    #[tokio::test]
    async fn test_handle_guest_key_event_full_submits_prompt_and_abort() {
        let (send, mut recv) = duplex(1024);
        let mut writer = CollabWriter::new(send, CapabilityLevel::Full);
        let renderer = TerminalRenderer::default();
        let mut prompt_buf = String::new();
        let mut pending_approval = None;

        let key_h = make_key(KeyCode::Char('h'), KeyModifiers::empty());
        let key_i = make_key(KeyCode::Char('i'), KeyModifiers::empty());
        handle_guest_key_event(
            key_h,
            CapabilityLevel::Full,
            &mut prompt_buf,
            &mut pending_approval,
            &mut writer,
            &renderer,
        )
        .await
        .unwrap();
        handle_guest_key_event(
            key_i,
            CapabilityLevel::Full,
            &mut prompt_buf,
            &mut pending_approval,
            &mut writer,
            &renderer,
        )
        .await
        .unwrap();
        assert_eq!(prompt_buf, "hi");

        let key_enter = make_key(KeyCode::Enter, KeyModifiers::empty());
        handle_guest_key_event(
            key_enter,
            CapabilityLevel::Full,
            &mut prompt_buf,
            &mut pending_approval,
            &mut writer,
            &renderer,
        )
        .await
        .unwrap();
        assert!(prompt_buf.is_empty());

        let mut lines_reader =
            rho_harness_core::rpc::transport::JsonLinesReader::new(tokio::io::BufReader::new(&mut recv));
        let cmd: Option<RpcCommand> = lines_reader.read_message().await.unwrap();
        assert!(matches!(cmd, Some(RpcCommand::Prompt { message, .. }) if message == "hi"));

        let key_esc = make_key(KeyCode::Esc, KeyModifiers::empty());
        handle_guest_key_event(
            key_esc,
            CapabilityLevel::Full,
            &mut prompt_buf,
            &mut pending_approval,
            &mut writer,
            &renderer,
        )
        .await
        .unwrap();
        let abort_cmd: Option<RpcCommand> = lines_reader.read_message().await.unwrap();
        assert!(matches!(abort_cmd, Some(RpcCommand::Abort)));
    }

    #[test]
    fn test_handle_incoming_rpc_event_dispatch() {
        let renderer = TerminalRenderer::default();
        let mut pending = None;
        assert!(handle_incoming_rpc_event(
            &RpcEvent::TextChunk {
                content: "stream".into()
            },
            &renderer,
            &mut pending,
        ));
        assert!(handle_incoming_rpc_event(
            &RpcEvent::ReasoningChunk {
                content: "think".into()
            },
            &renderer,
            &mut pending,
        ));
        assert!(handle_incoming_rpc_event(
            &RpcEvent::TurnStart {
                turn_number: 1,
                prompt: "start".into()
            },
            &renderer,
            &mut pending,
        ));
        assert!(handle_incoming_rpc_event(
            &RpcEvent::TurnEnd {
                stop_reason: "done".into(),
            },
            &renderer,
            &mut pending,
        ));
        assert!(!handle_incoming_rpc_event(
            &RpcEvent::Error {
                code: "KICKED".into(),
                message: "kicked".into()
            },
            &renderer,
            &mut pending,
        ));
        assert!(!handle_incoming_rpc_event(
            &RpcEvent::Error {
                code: "ROTATED".into(),
                message: "rotated".into()
            },
            &renderer,
            &mut pending,
        ));
        assert!(!handle_incoming_rpc_event(
            &RpcEvent::Error {
                code: "STOPPED".into(),
                message: "stopped".into()
            },
            &renderer,
            &mut pending,
        ));
        assert!(handle_incoming_rpc_event(
            &RpcEvent::Error {
                code: "UNAUTHORIZED".into(),
                message: "not allowed".into()
            },
            &renderer,
            &mut pending,
        ));
    }

    #[tokio::test]
    async fn test_run_guest_session_termination_on_host_disconnect() {
        let (host_send, guest_recv) = duplex(1024);
        let (guest_send, _host_recv) = duplex(1024);

        let writer = CollabWriter::new(guest_send, CapabilityLevel::Full);
        let reader = CollabReader::new(guest_recv, CapabilityLevel::Full);
        let renderer = TerminalRenderer::default();
        let input = TerminalInputReader::spawn_with_events(vec![]);

        drop(host_send);

        let res = run_guest_session(writer, reader, &renderer, input).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_guest_tool_approval_request_and_allow_deny() {
        let (send, mut recv) = duplex(1024);
        let mut writer = CollabWriter::new(send, CapabilityLevel::Full);
        let renderer = TerminalRenderer::default();
        let mut prompt_buf = String::new();
        let mut pending_approval = None;

        let approval_event = RpcEvent::ToolApprovalRequest {
            approval_id: "app-123".into(),
            tool: "bash".into(),
            arguments: serde_json::json!({"command": "ls"}),
            description: None,
        };
        assert!(handle_incoming_rpc_event(
            &approval_event,
            &renderer,
            &mut pending_approval
        ));
        assert_eq!(pending_approval.as_deref(), Some("app-123"));

        let key_y = make_key(KeyCode::Char('y'), KeyModifiers::empty());
        handle_guest_key_event(
            key_y,
            CapabilityLevel::Full,
            &mut prompt_buf,
            &mut pending_approval,
            &mut writer,
            &renderer,
        )
        .await
        .unwrap();
        assert!(pending_approval.is_none());

        let mut lines_reader =
            rho_harness_core::rpc::transport::JsonLinesReader::new(tokio::io::BufReader::new(&mut recv));
        let cmd: Option<RpcCommand> = lines_reader.read_message().await.unwrap();
        assert!(matches!(
            cmd,
            Some(RpcCommand::ToolResponse { approval_id, decision }) if approval_id == "app-123" && decision == "allow"
        ));

        let resolved_event = RpcEvent::ToolApprovalResolved {
            approval_id: "app-123".into(),
            decision: Some("allowed".into()),
        };
        assert!(handle_incoming_rpc_event(
            &resolved_event,
            &renderer,
            &mut pending_approval
        ));
        assert!(pending_approval.is_none());
    }

    #[test]
    fn test_print_connection_banner() {
        let renderer = TerminalRenderer::default();
        let snap = CollabSnapshot::new(vec![serde_json::json!({"turn": 1})], "idle");
        print_connection_banner(&renderer, CapabilityLevel::Full, Some(&snap));
        print_connection_banner(&renderer, CapabilityLevel::ViewOnly, None);
    }
}
