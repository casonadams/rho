use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::ModalKeyResult;

fn format_server_option(name: &str, server: &rho_harness_core::config::McpServerConfig) -> ModalOption {
    let transport_str = match server.resolved_transport() {
        rho_harness_core::config::McpTransportKind::Stdio => "stdio",
        rho_harness_core::config::McpTransportKind::StreamableHttp => "http",
        rho_harness_core::config::McpTransportKind::Sse => "sse",
    };

    let target = server
        .command
        .as_deref()
        .or(server.url.as_deref())
        .unwrap_or("<unspecified>");

    let active_mark = if server.enabled { "  ✓" } else { "  (off)" };
    let desc = format!("[{transport_str}] {target}{active_mark}");

    ModalOption::new(format!("{name:16}"), Some(desc))
}

fn build_mcp_options(session: &ReplSession) -> (Vec<ModalOption>, usize) {
    let mut options = Vec::new();
    for (name, server) in &session.config.mcp.servers {
        options.push(format_server_option(name, server));
    }

    if options.is_empty() {
        options.push(ModalOption::new(
            "none",
            Some("No MCP servers configured (add to config.toml or .mcp.json)".to_string()),
        ));
    }

    (options, 0)
}

pub fn open_mcp_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    let (options, initial_selection) = build_mcp_options(session);
    let mut modal = ModalState::new("Model Context Protocol", "", options).with_search(true);
    modal.selected = initial_selection;
    controller.state_mut().push_modal(modal);
}

fn extract_selected_server<B: TerminalBackend>(controller: &TerminalController<B>) -> Option<String> {
    let opt = controller.state().active_modal().and_then(|m| m.selected_option())?;
    let raw = opt.label.trim();
    if raw.eq_ignore_ascii_case("none") {
        None
    } else {
        Some(raw.to_string())
    }
}

fn pop_and_toggle<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    let selected = extract_selected_server(controller);
    controller.state_mut().pop_modal();
    controller.redraw()?;
    Ok(match selected {
        Some(server) => ModalKeyResult::McpServerToggled { server },
        None => ModalKeyResult::Handled,
    })
}

fn pop_and_cancel<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    controller.state_mut().pop_modal();
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}

fn apply_mcp_filter<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    character: Option<char>,
) -> Result<ModalKeyResult> {
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        let mut query = modal.filter_query.clone();
        if let Some(c) = character {
            query.push(c);
        } else {
            query.pop();
        }
        modal.set_filter(&query);
    }
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}

fn clear_filter_or_cancel<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    let has_filter = controller
        .state()
        .active_modal()
        .is_some_and(|m| !m.filter_query.is_empty());
    if has_filter {
        if let Some(modal) = controller.state_mut().active_modal_mut() {
            modal.set_filter("");
        }
        controller.redraw()?;
        return Ok(ModalKeyResult::Handled);
    }
    pop_and_cancel(controller)
}

fn handle_nav_or_filter<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: &KeyEvent,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Up | KeyCode::BackTab => {
            controller.state_mut().select_previous_modal_option();
            controller.redraw()?;
        }
        KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
            controller.state_mut().select_previous_modal_option();
            controller.redraw()?;
        }
        KeyCode::Down | KeyCode::Tab => {
            controller.state_mut().select_next_modal_option();
            controller.redraw()?;
        }
        KeyCode::Backspace => return apply_mcp_filter(controller, None),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return clear_filter_or_cancel(controller);
        }
        KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
            return apply_mcp_filter(controller, Some(c));
        }
        _ => {}
    }
    Ok(ModalKeyResult::Handled)
}

pub fn handle_mcp_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Enter => pop_and_toggle(controller),
        KeyCode::Esc => pop_and_cancel(controller),
        _ => handle_nav_or_filter(controller, &key),
    }
}
