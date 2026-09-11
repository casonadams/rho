use super::common::HistoryTerminal;
use crate::repl::ReplSession;
use crate::repl::live::modal::{ModalKeyResult, handle_modal_key, open_mcp_selector};
use crate::ui::interactive::{InteractiveState, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_harness_core::config::{Config, McpConfig, McpServerConfig};
use std::collections::BTreeMap;

fn send_modal_key(c: &mut TerminalController<HistoryTerminal>, code: KeyCode) -> ModalKeyResult {
    handle_modal_key(c, KeyEvent::new(code, KeyModifiers::NONE), &mut None).unwrap()
}

fn setup_mcp_controller() -> TerminalController<HistoryTerminal> {
    let mut servers = BTreeMap::new();
    servers.insert(
        "filesystem".to_string(),
        McpServerConfig::stdio("npx", vec!["-y".to_string()]),
    );
    let remote = McpServerConfig {
        url: Some("https://example.com/mcp".to_string()),
        enabled: false,
        ..Default::default()
    };
    servers.insert("remote_tool".to_string(), remote);

    let config = Config {
        mcp: McpConfig { enabled: true, servers },
        ..Default::default()
    };
    let session = ReplSession::new(config, crate::auth::AuthStore::default(), None);
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    open_mcp_selector(&session, &mut controller);
    controller
}

#[test]
fn mcp_selector_opens_with_configured_servers() {
    let controller = setup_mcp_controller();
    let modal = controller.state().active_modal().unwrap();
    assert_eq!(modal.title, "Model Context Protocol");
    assert_eq!(modal.options.len(), 2);
}

#[test]
fn mcp_selector_marks_active_servers() {
    let controller = setup_mcp_controller();
    let modal = controller.state().active_modal().unwrap();
    assert!(modal.options[0].description.as_deref().unwrap().contains('✓'));
    assert!(modal.options[1].description.as_deref().unwrap().contains("(off)"));
}

#[test]
fn mcp_selector_navigates_and_toggles() {
    let mut controller = setup_mcp_controller();
    let _ = send_modal_key(&mut controller, KeyCode::Down);
    assert_eq!(controller.state().active_modal().unwrap().selected, 1);

    let res = send_modal_key(&mut controller, KeyCode::Enter);
    assert_eq!(
        res,
        ModalKeyResult::McpServerToggled {
            server: "remote_tool".to_string()
        }
    );
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn mcp_selector_cancels_on_esc() {
    let mut controller = setup_mcp_controller();
    let res = send_modal_key(&mut controller, KeyCode::Esc);
    assert_eq!(res, ModalKeyResult::Handled);
    assert!(controller.state().active_modal().is_none());
}
