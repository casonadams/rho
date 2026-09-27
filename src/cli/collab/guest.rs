use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use iroh::Endpoint;
use iroh::endpoint::presets::N0;
use rho_harness_core::collab::protocol::COLLAB_ALPN;
use rho_harness_core::collab::session::{CollabReader, CollabSessionStream, CollabWriter};
use rho_harness_core::collab::{CapabilityLevel, CollabSnapshot, CollabTicket};
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::rpc::protocol::{RpcCommand, RpcEvent};
use std::io::IsTerminal;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::repl::input_reader::TerminalInputReader;
use crate::repl::live::batch::{LiveBatch, OUTPUT_FRAME_INTERVAL};
use crate::repl::live::modal::{PendingModal, handle_modal_key, install_interaction};
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{
    Activity, InputAction, InteractionOption, InteractionPrompt, InteractionResponder, InteractionResponse,
    InteractiveState, InteractiveUi, OptionLayout, TerminalBackend, TerminalController, TranscriptItem, UiAction,
    UiEvent, map_key,
};

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

    let (writer, reader, snapshot) = connect_and_handshake(&endpoint, &ticket).await?;

    dispatch_guest_session(writer, reader, snapshot).await?;
    Ok(())
}

fn is_interactive_terminal() -> bool {
    crate::repl::live::live_ui_supported(std::io::stdin().is_terminal(), std::io::stdout().is_terminal())
}

async fn dispatch_guest_session<W: AsyncWrite + Unpin + Send + 'static, R: AsyncRead + Unpin + Send + 'static>(
    writer: CollabWriter<W>,
    reader: CollabReader<R>,
    snapshot: Option<CollabSnapshot>,
) -> Result<()> {
    if is_interactive_terminal() {
        run_guest_interactive(writer, reader, snapshot).await
    } else {
        run_guest_stream_fallback(writer, reader, snapshot).await
    }
}

async fn run_guest_stream_fallback<W: AsyncWrite + Unpin + Send + 'static, R: AsyncRead + Unpin + Send + 'static>(
    writer: CollabWriter<W>,
    reader: CollabReader<R>,
    snapshot: Option<CollabSnapshot>,
) -> Result<()> {
    let renderer = TerminalRenderer::default();
    print_connection_banner(&renderer, reader.role(), snapshot.as_ref());
    let input = TerminalInputReader::spawn()?;
    let _raw_guard = RawModeGuard::enter()?;
    run_guest_session(writer, reader, &renderer, input).await
}

pub(crate) async fn run_guest_interactive<
    W: AsyncWrite + Unpin + Send + 'static,
    R: AsyncRead + Unpin + Send + 'static,
>(
    writer: CollabWriter<W>,
    reader: CollabReader<R>,
    snapshot: Option<CollabSnapshot>,
) -> Result<()> {
    let (ui, ui_events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);
    let role = reader.role();

    let mut state = InteractiveState::default();
    configure_guest_state(&mut state, role);
    let mut controller = TerminalController::stdout(state)?;

    if let Some(ref snap) = snapshot {
        let _ = hydrate_snapshot_controller(&mut controller, snap);
    }

    let input = TerminalInputReader::spawn()?;
    run_guest_interactive_loop(writer, reader, &renderer, &mut controller, ui_events, input).await
}

pub fn configure_guest_state(state: &mut InteractiveState, role: CapabilityLevel) {
    let role_label = match role {
        CapabilityLevel::Full => "co-pilot",
        CapabilityLevel::ViewOnly => "spectator",
    };
    let footer = state.footer_mut();
    footer.provider = "collab".to_string();
    footer.model = role_label.to_string();
    footer.remote_active = true;
    footer.extra_status = Some(format!("[Collab: {role_label}]"));
    if role == CapabilityLevel::ViewOnly {
        state.set_system_message(Some("View Only - Read Mode".to_string()));
    }
}

pub fn hydrate_snapshot_controller<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    snapshot: &CollabSnapshot,
) -> std::io::Result<()> {
    for turn_val in &snapshot.turns {
        if let Ok(turn) = serde_json::from_value::<rho_harness_core::session::turns::ConversationTurn>(turn_val.clone())
        {
            if !turn.user_prompt.is_empty() {
                controller.push_transcript_item(TranscriptItem::UserMessage(turn.user_prompt))?;
            }
            if !turn.assistant_preview.is_empty() {
                controller.push_transcript_item(TranscriptItem::AssistantText(turn.assistant_preview))?;
            }
        }
    }
    controller.redraw()?;
    Ok(())
}

#[derive(Default)]
pub struct GuestApprovalState {
    pub current_id: Option<String>,
    pub modal_pending: Option<PendingModal>,
    pub response_rx: Option<tokio::sync::oneshot::Receiver<InteractionResponse>>,
}

pub(crate) async fn run_guest_interactive_loop<B: TerminalBackend, W: AsyncWrite + Unpin, R: AsyncRead + Unpin>(
    mut writer: CollabWriter<W>,
    mut reader: CollabReader<R>,
    renderer: &TerminalRenderer,
    controller: &mut TerminalController<B>,
    mut ui_events: tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    mut input: TerminalInputReader,
) -> Result<()> {
    let role = reader.role();
    let mut batch = LiveBatch::new();
    let mut approvals = GuestApprovalState::default();
    let mut interval = tokio::time::interval(OUTPUT_FRAME_INTERVAL);

    loop {
        tokio::select! {
            event_res = reader.recv_event() => {
                match event_res {
                    Ok(Some(event)) => {
                        let keep_running = handle_guest_rpc_event(
                            &event,
                            renderer,
                            controller,
                            &mut approvals,
                        ).await?;
                        if !keep_running {
                            break;
                        }
                    }
                    Ok(None) => {
                        controller.set_system_message("Host disconnected");
                        break;
                    }
                    Err(e) => {
                        controller.set_system_message(format!("Connection error: {e}"));
                        break;
                    }
                }
            }
            Some(Ok(term_event)) = input.recv() => {
                let exit = handle_guest_terminal_event(
                    term_event,
                    controller,
                    &mut writer,
                    role,
                    &mut approvals,
                ).await?;
                if exit {
                    break;
                }
            }
            _ = interval.tick() => {
                drain_guest_ui_tick(
                    controller,
                    &mut batch,
                    &mut ui_events,
                    &mut writer,
                    &mut approvals,
                ).await?;
            }
        }
    }

    let _ = controller.redraw();
    Ok(())
}

async fn drain_guest_ui_tick<B: TerminalBackend, W: AsyncWrite + Unpin>(
    controller: &mut TerminalController<B>,
    batch: &mut LiveBatch,
    ui_events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    writer: &mut CollabWriter<W>,
    approvals: &mut GuestApprovalState,
) -> Result<()> {
    while let Ok(event) = ui_events.try_recv() {
        batch.push_event(controller, event)?;
    }
    batch.flush(controller, false)?;
    controller.check_system_message_expiration();

    poll_and_send_approval_response(writer, approvals).await
}

async fn poll_and_send_approval_response<W: AsyncWrite + Unpin>(
    writer: &mut CollabWriter<W>,
    approvals: &mut GuestApprovalState,
) -> Result<()> {
    let Some(rx) = &mut approvals.response_rx else {
        return Ok(());
    };
    let Ok(resp) = rx.try_recv() else {
        return Ok(());
    };
    let decision = match resp {
        InteractionResponse::Selected(0) => "allow",
        _ => "deny",
    };
    if let Some(id) = approvals.current_id.take() {
        writer
            .send_command(&RpcCommand::ToolResponse {
                approval_id: id,
                decision: decision.to_string(),
            })
            .await?;
    }
    approvals.response_rx = None;
    approvals.modal_pending = None;
    Ok(())
}

pub fn apply_guest_usage_update(footer: &mut crate::ui::interactive::FooterState, event: &RpcEvent) {
    let RpcEvent::UsageUpdate {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        total_cost,
        context_percent,
        context_window,
        tokens_per_second,
        quota,
    } = event
    else {
        return;
    };
    if let Some(tok) = input_tokens {
        footer.total_input_tokens = *tok;
    }
    if let Some(tok) = output_tokens {
        footer.total_output_tokens = *tok;
    }
    if let Some(tok) = cache_read_tokens {
        footer.total_cache_read_tokens = *tok;
    }
    if let Some(tok) = cache_write_tokens {
        footer.total_cache_write_tokens = *tok;
    }
    if total_cost.is_some() {
        footer.total_cost = *total_cost;
    }
    if context_percent.is_some() {
        footer.context_percent = *context_percent;
    }
    if let Some(cw) = context_window {
        footer.context_window = *cw;
    }
    if tokens_per_second.is_some() {
        footer.tokens_per_second = *tokens_per_second;
    }
    if quota.is_some() {
        footer.quota = quota.clone();
    }
}

pub async fn handle_guest_rpc_event<B: TerminalBackend>(
    event: &RpcEvent,
    renderer: &TerminalRenderer,
    controller: &mut TerminalController<B>,
    approvals: &mut GuestApprovalState,
) -> Result<bool> {
    match event {
        RpcEvent::SessionStart { model, provider, .. } => {
            let footer = controller.state_mut().footer_mut();
            if !model.is_empty() {
                footer.model = model.clone();
            }
            if !provider.is_empty() {
                footer.provider = provider.clone();
            }
        }
        RpcEvent::TurnStart { prompt, .. } => {
            controller.commit_streamed_output();
            renderer.print_user_block(prompt);
            renderer.flush();
            controller.state_mut().footer_mut().activity = Activity::Working;
        }
        RpcEvent::TextChunk { content } => {
            renderer.print_token(content);
        }
        RpcEvent::ReasoningChunk { content } => {
            renderer.print_thinking_token(content);
        }
        RpcEvent::TurnEnd { .. } => {
            renderer.write_output("\n\n");
            renderer.flush();
            controller.commit_streamed_output();
            controller.state_mut().footer_mut().activity = Activity::Idle;
        }
        RpcEvent::UsageUpdate { .. } => {
            apply_guest_usage_update(controller.state_mut().footer_mut(), event);
        }
        RpcEvent::ToolCallStart { tool, arguments, .. } => {
            renderer.start_tool_run(tool, arguments);
        }
        RpcEvent::ToolCallResult {
            tool, output, is_error, ..
        } => {
            handle_tool_call_result(renderer, tool, output, *is_error);
        }
        RpcEvent::StatusChanged { status } => {
            renderer.set_extra_status(Some(status.clone()));
        }
        RpcEvent::ToolApprovalRequest {
            approval_id,
            tool,
            arguments,
            description,
        } => {
            install_guest_approval_modal(
                controller,
                approval_id,
                tool,
                arguments,
                description.as_deref(),
                approvals,
            );
        }
        RpcEvent::ToolApprovalResolved { approval_id, .. } => {
            dismiss_guest_approval_modal(controller, approval_id, approvals);
        }
        RpcEvent::Error { code, message } => {
            if matches!(code.as_str(), "KICKED" | "ROTATED" | "STOPPED") {
                controller.set_system_message(format!("{code}: {message}"));
                return Ok(false);
            }
            renderer.print_notice(&format!("\n● Error [{code}]: {message}\n"));
        }
        _ => {}
    }
    Ok(true)
}

fn handle_tool_call_result(renderer: &TerminalRenderer, tool: &str, output: &str, is_error: bool) {
    renderer.finish_tool_line(rho_harness_core::presentation::ToolLine {
        name: tool.to_string(),
        arguments: serde_json::Value::Null,
        output: output.to_string(),
        output_summary: String::new(),
        is_error,
        duration_ms: None,
    });
}

pub fn install_guest_approval_modal<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    approval_id: &str,
    tool: &str,
    arguments: &serde_json::Value,
    description: Option<&str>,
    approvals: &mut GuestApprovalState,
) {
    let body = arguments
        .get("body")
        .and_then(|v| v.as_str())
        .or(description)
        .unwrap_or("Tool execution approval required");
    let options = vec![
        InteractionOption {
            label: "Allow".to_string(),
            description: Some("Approve tool execution".to_string()),
            input: None,
        },
        InteractionOption {
            label: "Deny".to_string(),
            description: Some("Reject tool execution".to_string()),
            input: None,
        },
    ];
    let prompt = InteractionPrompt {
        title: format!("Approval Required: {tool}"),
        body: body.to_string(),
        options,
        initial_selection: 0,
        allow_custom: false,
        initial_text: None,
        option_layout: OptionLayout::default(),
    };
    let (responder_tx, responder_rx) = tokio::sync::oneshot::channel();
    let event = UiEvent::Interaction {
        prompt,
        responder: InteractionResponder {
            responder: responder_tx,
        },
    };
    install_interaction(controller, event, &mut approvals.modal_pending);
    approvals.current_id = Some(approval_id.to_string());
    approvals.response_rx = Some(responder_rx);
}

pub fn dismiss_guest_approval_modal<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    approval_id: &str,
    approvals: &mut GuestApprovalState,
) {
    if approvals.current_id.as_deref() == Some(approval_id) {
        approvals.current_id = None;
        approvals.modal_pending = None;
        approvals.response_rx = None;
        if controller.state().active_modal().is_some() {
            controller.state_mut().pop_modal();
        }
    }
}

pub async fn handle_guest_terminal_event<B: TerminalBackend, W: AsyncWrite + Unpin>(
    event: Event,
    controller: &mut TerminalController<B>,
    writer: &mut CollabWriter<W>,
    role: CapabilityLevel,
    approvals: &mut GuestApprovalState,
) -> Result<bool> {
    match event {
        Event::Resize(cols, rows) => {
            controller.resize_to(cols as usize, rows as usize)?;
            Ok(false)
        }
        Event::Paste(text) => {
            if role == CapabilityLevel::Full && controller.state().active_modal().is_none() {
                controller.state_mut().editor_mut().handle_paste(&text);
                controller.redraw()?;
            }
            Ok(false)
        }
        Event::Key(key) => {
            if key.kind == KeyEventKind::Release {
                return Ok(false);
            }
            if controller.state().active_modal().is_some() {
                handle_modal_key_event(key, controller, writer, role, approvals).await
            } else if role == CapabilityLevel::ViewOnly {
                handle_spectator_key_event(key, controller)
            } else {
                handle_copilot_key_event(key, controller, writer).await
            }
        }
        _ => Ok(false),
    }
}

async fn handle_modal_key_event<B: TerminalBackend, W: AsyncWrite + Unpin>(
    key: KeyEvent,
    controller: &mut TerminalController<B>,
    writer: &mut CollabWriter<W>,
    role: CapabilityLevel,
    approvals: &mut GuestApprovalState,
) -> Result<bool> {
    if role != CapabilityLevel::Full {
        if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
            return Ok(true);
        }
        return Ok(false);
    }

    if matches!(key.code, KeyCode::Char('y' | 'Y')) {
        return send_quick_approval(controller, writer, approvals, "allow").await;
    }
    if matches!(key.code, KeyCode::Char('n' | 'N')) {
        return send_quick_approval(controller, writer, approvals, "deny").await;
    }

    let _ = handle_modal_key(controller, key, &mut approvals.modal_pending)?;
    controller.redraw()?;
    Ok(false)
}

async fn send_quick_approval<B: TerminalBackend, W: AsyncWrite + Unpin>(
    controller: &mut TerminalController<B>,
    writer: &mut CollabWriter<W>,
    approvals: &mut GuestApprovalState,
    decision: &str,
) -> Result<bool> {
    if let Some(id) = approvals.current_id.take() {
        approvals.modal_pending = None;
        approvals.response_rx = None;
        if controller.state().active_modal().is_some() {
            controller.state_mut().pop_modal();
        }
        writer
            .send_command(&RpcCommand::ToolResponse {
                approval_id: id,
                decision: decision.to_string(),
            })
            .await?;
        controller.redraw()?;
    }
    Ok(false)
}

pub fn handle_spectator_key_event<B: TerminalBackend>(
    key: KeyEvent,
    controller: &mut TerminalController<B>,
) -> Result<bool> {
    let is_ctrl_c = key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
    let is_ctrl_d = key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL);
    if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) || is_ctrl_c || is_ctrl_d {
        return Ok(true);
    }
    controller.set_system_message("View Only - Read Mode");
    controller.redraw()?;
    Ok(false)
}

pub async fn handle_copilot_key_event<B: TerminalBackend, W: AsyncWrite + Unpin>(
    key: KeyEvent,
    controller: &mut TerminalController<B>,
    writer: &mut CollabWriter<W>,
) -> Result<bool> {
    match map_key(key) {
        InputAction::Cancel => {
            writer.send_command(&RpcCommand::Abort).await?;
        }
        InputAction::Clear => {
            controller.state_mut().editor_mut().set_text("");
            controller.redraw()?;
        }
        InputAction::EndOfInput => {
            if controller.state().editor().is_empty() {
                return Ok(true);
            }
        }
        InputAction::ThinkingToggle => {
            let curr = controller.state().hide_thinking();
            controller.state_mut().set_hide_thinking(!curr);
            controller.redraw()?;
        }
        InputAction::ToggleExpandTools => {
            let curr = controller.state().tools_expanded();
            controller.state_mut().set_tools_expanded(!curr);
            controller.redraw()?;
        }
        InputAction::Edit(UiAction::Submit(_)) => {
            submit_copilot_prompt(controller, writer).await?;
        }
        InputAction::Edit(action) => {
            controller.state_mut().apply(action);
            controller.redraw()?;
        }
        _ => {}
    }
    Ok(false)
}

async fn submit_copilot_prompt<B: TerminalBackend, W: AsyncWrite + Unpin>(
    controller: &mut TerminalController<B>,
    writer: &mut CollabWriter<W>,
) -> Result<()> {
    let text = controller.state().editor().text().trim().to_string();
    if !text.is_empty() {
        controller.state_mut().editor_mut().set_text("");
        writer
            .send_command(&RpcCommand::Prompt {
                message: text,
                images: None,
                streaming_behavior: None,
            })
            .await?;
        controller.redraw()?;
    }
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

    struct MockBackend;
    impl TerminalBackend for MockBackend {
        fn set_raw_mode(&mut self, _: bool) -> std::io::Result<()> {
            Ok(())
        }
        fn size(&self) -> std::io::Result<(u16, u16)> {
            Ok((80, 24))
        }
        fn hide_cursor(&mut self) -> std::io::Result<()> {
            Ok(())
        }
        fn show_cursor(&mut self) -> std::io::Result<()> {
            Ok(())
        }
        fn move_up(&mut self, _: usize) -> std::io::Result<()> {
            Ok(())
        }
        fn move_down(&mut self, _: usize) -> std::io::Result<()> {
            Ok(())
        }
        fn move_to_column(&mut self, _: usize) -> std::io::Result<()> {
            Ok(())
        }
        fn clear_line(&mut self) -> std::io::Result<()> {
            Ok(())
        }
        fn write_text(&mut self, _: &str) -> std::io::Result<()> {
            Ok(())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_configure_guest_state_modes() {
        let mut state_full = InteractiveState::default();
        configure_guest_state(&mut state_full, CapabilityLevel::Full);
        assert_eq!(state_full.footer().model, "co-pilot");
        assert_eq!(state_full.footer().extra_status.as_deref(), Some("[Collab: co-pilot]"));
        assert!(state_full.footer().remote_active);

        let mut state_view = InteractiveState::default();
        configure_guest_state(&mut state_view, CapabilityLevel::ViewOnly);
        assert_eq!(state_view.footer().model, "spectator");
        assert_eq!(state_view.footer().extra_status.as_deref(), Some("[Collab: spectator]"));
        assert_eq!(state_view.system_message(), Some("View Only - Read Mode"));
    }

    #[test]
    fn test_hydrate_snapshot_controller_populates_transcript() {
        let mut controller = TerminalController::new(MockBackend, InteractiveState::default()).unwrap();
        let turn = rho_harness_core::session::turns::ConversationTurn {
            turn_number: 1,
            user_prompt: "hello from past".into(),
            assistant_preview: "answer from past".into(),
            tool_calls_count: 0,
        };
        let snapshot = CollabSnapshot::new(vec![serde_json::to_value(turn).unwrap()], "idle");
        hydrate_snapshot_controller(&mut controller, &snapshot).unwrap();

        assert_eq!(controller.transcript().len(), 2);
        assert!(matches!(
            controller.transcript()[0],
            TranscriptItem::UserMessage(ref msg) if msg == "hello from past"
        ));
        assert!(matches!(
            controller.transcript()[1],
            TranscriptItem::AssistantText(ref msg) if msg == "answer from past"
        ));
    }

    #[tokio::test]
    async fn test_handle_copilot_key_event_workflow() {
        let (send, mut recv) = duplex(1024);
        let mut writer = CollabWriter::new(send, CapabilityLevel::Full);
        let mut controller = TerminalController::new(MockBackend, InteractiveState::default()).unwrap();

        let key_h = make_key(KeyCode::Char('h'), KeyModifiers::empty());
        let key_i = make_key(KeyCode::Char('i'), KeyModifiers::empty());
        handle_copilot_key_event(key_h, &mut controller, &mut writer)
            .await
            .unwrap();
        handle_copilot_key_event(key_i, &mut controller, &mut writer)
            .await
            .unwrap();
        assert_eq!(controller.state().editor().text(), "hi");

        let key_enter = make_key(KeyCode::Enter, KeyModifiers::empty());
        handle_copilot_key_event(key_enter, &mut controller, &mut writer)
            .await
            .unwrap();
        assert!(controller.state().editor().is_empty());

        let mut lines_reader =
            rho_harness_core::rpc::transport::JsonLinesReader::new(tokio::io::BufReader::new(&mut recv));
        let cmd: Option<RpcCommand> = lines_reader.read_message().await.unwrap();
        assert!(matches!(cmd, Some(RpcCommand::Prompt { message, .. }) if message == "hi"));

        let key_esc = make_key(KeyCode::Esc, KeyModifiers::empty());
        handle_copilot_key_event(key_esc, &mut controller, &mut writer)
            .await
            .unwrap();
        let abort_cmd: Option<RpcCommand> = lines_reader.read_message().await.unwrap();
        assert!(matches!(abort_cmd, Some(RpcCommand::Abort)));

        controller.state_mut().editor_mut().set_text("draft to clear");
        let key_ctrl_c = make_key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        handle_copilot_key_event(key_ctrl_c, &mut controller, &mut writer)
            .await
            .unwrap();
        assert!(controller.state().editor().is_empty());

        let key_ctrl_d = make_key(KeyCode::Char('d'), KeyModifiers::CONTROL);
        let exit = handle_copilot_key_event(key_ctrl_d, &mut controller, &mut writer)
            .await
            .unwrap();
        assert!(exit);
    }

    #[test]
    fn test_handle_spectator_key_event_behavior() {
        let mut controller = TerminalController::new(MockBackend, InteractiveState::default()).unwrap();

        let key_q = make_key(KeyCode::Char('q'), KeyModifiers::empty());
        assert!(handle_spectator_key_event(key_q, &mut controller).unwrap());

        let key_esc = make_key(KeyCode::Esc, KeyModifiers::empty());
        assert!(handle_spectator_key_event(key_esc, &mut controller).unwrap());

        let key_ctrl_c = make_key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(handle_spectator_key_event(key_ctrl_c, &mut controller).unwrap());

        let key_ctrl_d = make_key(KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert!(handle_spectator_key_event(key_ctrl_d, &mut controller).unwrap());

        let key_a = make_key(KeyCode::Char('a'), KeyModifiers::empty());
        assert!(!handle_spectator_key_event(key_a, &mut controller).unwrap());
        assert_eq!(controller.state().system_message(), Some("View Only - Read Mode"));
    }

    #[test]
    fn test_apply_guest_usage_update() {
        let mut footer = crate::ui::interactive::FooterState::default();
        let usage = RpcEvent::UsageUpdate {
            input_tokens: Some(1500),
            output_tokens: Some(300),
            cache_read_tokens: Some(120),
            cache_write_tokens: Some(40),
            total_cost: Some(0.042),
            context_percent: Some(25.5),
            context_window: Some(200_000),
            tokens_per_second: Some(45.0),
            quota: Some("85%".to_string()),
        };
        apply_guest_usage_update(&mut footer, &usage);
        assert_eq!(footer.total_input_tokens, 1500);
        assert_eq!(footer.total_output_tokens, 300);
        assert_eq!(footer.total_cache_read_tokens, 120);
        assert_eq!(footer.total_cache_write_tokens, 40);
        assert_eq!(footer.total_cost, Some(0.042));
        assert_eq!(footer.context_percent, Some(25.5));
        assert_eq!(footer.context_window, 200_000);
        assert_eq!(footer.tokens_per_second, Some(45.0));
        assert_eq!(footer.quota, Some("85%".to_string()));

        let non_usage = RpcEvent::StatusChanged { status: "ready".into() };
        apply_guest_usage_update(&mut footer, &non_usage);
        assert_eq!(footer.total_input_tokens, 1500);
    }

    #[tokio::test]
    async fn test_handle_guest_rpc_event_session_and_usage_lifecycle() {
        let (ui, _rx) = InteractiveUi::channel();
        let renderer = TerminalRenderer::with_ui(ui);
        let mut controller = TerminalController::new(MockBackend, InteractiveState::default()).unwrap();
        let mut approvals = GuestApprovalState::default();

        let session_start = RpcEvent::SessionStart {
            session_id: "s1".into(),
            model: "claude-3-7-sonnet".into(),
            provider: "anthropic".into(),
        };
        assert!(
            handle_guest_rpc_event(&session_start, &renderer, &mut controller, &mut approvals)
                .await
                .unwrap()
        );
        assert_eq!(controller.state().footer().model, "claude-3-7-sonnet");
        assert_eq!(controller.state().footer().provider, "anthropic");

        let turn_start = RpcEvent::TurnStart {
            turn_number: 1,
            prompt: "hello".into(),
        };
        assert!(
            handle_guest_rpc_event(&turn_start, &renderer, &mut controller, &mut approvals)
                .await
                .unwrap()
        );
        assert_eq!(controller.state().footer().activity, Activity::Working);

        let usage = RpcEvent::UsageUpdate {
            input_tokens: Some(400),
            output_tokens: Some(80),
            cache_read_tokens: None,
            cache_write_tokens: None,
            total_cost: None,
            context_percent: Some(10.0),
            context_window: Some(128_000),
            tokens_per_second: Some(25.0),
            quota: Some("95%".into()),
        };
        assert!(
            handle_guest_rpc_event(&usage, &renderer, &mut controller, &mut approvals)
                .await
                .unwrap()
        );
        assert_eq!(controller.state().footer().total_input_tokens, 400);
        assert_eq!(controller.state().footer().total_output_tokens, 80);

        let turn_end = RpcEvent::TurnEnd {
            stop_reason: "completed".into(),
        };
        assert!(
            handle_guest_rpc_event(&turn_end, &renderer, &mut controller, &mut approvals)
                .await
                .unwrap()
        );
        assert_eq!(controller.state().footer().activity, Activity::Idle);
    }

    #[tokio::test]
    async fn test_handle_guest_rpc_event_turn_and_tool() {
        let (ui, _rx) = InteractiveUi::channel();
        let renderer = TerminalRenderer::with_ui(ui);
        let mut controller = TerminalController::new(MockBackend, InteractiveState::default()).unwrap();
        controller.write_stream_output("pending output").unwrap();
        let mut approvals = GuestApprovalState::default();

        let turn_start = RpcEvent::TurnStart {
            turn_number: 1,
            prompt: "interactive prompt".into(),
        };
        assert!(
            handle_guest_rpc_event(&turn_start, &renderer, &mut controller, &mut approvals)
                .await
                .unwrap()
        );

        let text_chunk = RpcEvent::TextChunk {
            content: "interactive token".into(),
        };
        assert!(
            handle_guest_rpc_event(&text_chunk, &renderer, &mut controller, &mut approvals)
                .await
                .unwrap()
        );

        let status_event = RpcEvent::StatusChanged {
            status: "running tests".into(),
        };
        assert!(
            handle_guest_rpc_event(&status_event, &renderer, &mut controller, &mut approvals)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn test_handle_guest_rpc_event_approval_and_error() {
        let (ui, _rx) = InteractiveUi::channel();
        let renderer = TerminalRenderer::with_ui(ui);
        let mut controller = TerminalController::new(MockBackend, InteractiveState::default()).unwrap();
        let mut approvals = GuestApprovalState::default();

        let req = RpcEvent::ToolApprovalRequest {
            approval_id: "app-99".into(),
            tool: "bash".into(),
            arguments: serde_json::json!({"body": "Run rm -rf?"}),
            description: None,
        };
        assert!(
            handle_guest_rpc_event(&req, &renderer, &mut controller, &mut approvals)
                .await
                .unwrap()
        );
        assert_eq!(approvals.current_id.as_deref(), Some("app-99"));
        assert!(controller.state().active_modal().is_some());

        let resolved = RpcEvent::ToolApprovalResolved {
            approval_id: "app-99".into(),
            decision: Some("allow".into()),
        };
        assert!(
            handle_guest_rpc_event(&resolved, &renderer, &mut controller, &mut approvals)
                .await
                .unwrap()
        );
        assert_eq!(approvals.current_id, None);
        assert!(controller.state().active_modal().is_none());

        let kicked = RpcEvent::Error {
            code: "KICKED".into(),
            message: "kicked".into(),
        };
        let keep_running = handle_guest_rpc_event(&kicked, &renderer, &mut controller, &mut approvals)
            .await
            .unwrap();
        assert!(!keep_running);
    }

    #[tokio::test]
    async fn test_drain_guest_ui_tick_and_approval_resolution() {
        let (send, mut recv) = duplex(1024);
        let mut writer = CollabWriter::new(send, CapabilityLevel::Full);
        let mut controller = TerminalController::new(MockBackend, InteractiveState::default()).unwrap();
        let mut batch = LiveBatch::new();
        let (ui, mut ui_events) = InteractiveUi::channel();
        let _ = ui.set_extra_status(Some("testing".into()));

        let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
        let mut approvals = GuestApprovalState {
            current_id: Some("app-99".into()),
            modal_pending: None,
            response_rx: Some(resp_rx),
        };

        let _ = resp_tx.send(InteractionResponse::Selected(0));

        drain_guest_ui_tick(&mut controller, &mut batch, &mut ui_events, &mut writer, &mut approvals)
            .await
            .unwrap();

        assert!(approvals.current_id.is_none());
        assert!(approvals.response_rx.is_none());

        let mut lines_reader =
            rho_harness_core::rpc::transport::JsonLinesReader::new(tokio::io::BufReader::new(&mut recv));
        let cmd: Option<RpcCommand> = lines_reader.read_message().await.unwrap();
        assert!(matches!(
            cmd,
            Some(RpcCommand::ToolResponse {
                approval_id,
                decision
            }) if approval_id == "app-99" && decision == "allow"
        ));
    }

    #[tokio::test]
    async fn test_guest_terminal_quick_approval_workflow() {
        let (send, mut recv) = duplex(1024);
        let mut writer = CollabWriter::new(send, CapabilityLevel::Full);
        let mut controller = TerminalController::new(MockBackend, InteractiveState::default()).unwrap();
        let mut approvals = GuestApprovalState::default();

        install_guest_approval_modal(
            &mut controller,
            "app-42",
            "git",
            &serde_json::json!({}),
            Some("Allow git commit?"),
            &mut approvals,
        );
        assert!(controller.state().active_modal().is_some());

        let key_y = make_key(KeyCode::Char('y'), KeyModifiers::empty());
        let exit = handle_guest_terminal_event(
            Event::Key(key_y),
            &mut controller,
            &mut writer,
            CapabilityLevel::Full,
            &mut approvals,
        )
        .await
        .unwrap();
        assert!(!exit);
        assert!(controller.state().active_modal().is_none());
        assert_eq!(approvals.current_id, None);

        let mut lines_reader =
            rho_harness_core::rpc::transport::JsonLinesReader::new(tokio::io::BufReader::new(&mut recv));
        let cmd: Option<RpcCommand> = lines_reader.read_message().await.unwrap();
        assert!(matches!(
            cmd,
            Some(RpcCommand::ToolResponse { approval_id, decision }) if approval_id == "app-42" && decision == "allow"
        ));
    }
}
