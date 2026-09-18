use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent};

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

pub fn handle_mcp_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Enter => {
            let server = extract_selected_server(controller);
            super::pop_and_cancel(controller)?;
            Ok(match server {
                Some(server) => ModalKeyResult::McpServerToggled { server },
                None => ModalKeyResult::Handled,
            })
        }
        KeyCode::Esc => {
            super::pop_and_cancel(controller)?;
            Ok(ModalKeyResult::Handled)
        }
        _ => {
            super::handle_selector_nav(controller, &key)?;
            Ok(ModalKeyResult::Handled)
        }
    }
}
