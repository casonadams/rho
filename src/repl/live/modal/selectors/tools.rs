use crossterm::event::{KeyCode, KeyEvent};

use super::super::ModalKeyResult;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};

pub fn build_tools_options(session: &ReplSession) -> Vec<ModalOption> {
    let search_engine = &session.config.tools.web.search.default;
    let web_search = if session.config.tools.web.search.enabled {
        "On"
    } else {
        "Off"
    };
    let web_fetch = if session.config.tools.web.fetch.enabled {
        "On"
    } else {
        "Off"
    };
    let mcp = if session.config.mcp.enabled { "On" } else { "Off" };
    let permission = if session.config.permission.enabled { "On" } else { "Off" };

    vec![
        ModalOption::new("Search Engine     ", Some(search_engine.to_string())),
        ModalOption::new("Web Search        ", Some(web_search.to_string())),
        ModalOption::new("Web Fetch         ", Some(web_fetch.to_string())),
        ModalOption::new("MCP               ", Some(mcp.to_string())),
        ModalOption::new("LSP Diagnostics   ", Some("On".to_string())),
        ModalOption::new("Edit Mode         ", Some("Hashline".to_string())),
        ModalOption::new("Permissions       ", Some(permission.to_string())),
    ]
}

pub fn open_tools_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    let options = build_tools_options(session);
    let modal = ModalState::new("Tools & Permissions", "", options);
    controller.state_mut().push_modal(modal);
}

fn update_tools_option_description<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    status: &str,
    index: usize,
) {
    controller
        .state_mut()
        .update_modal_option_desc("Tools & Permissions", index, status);
}

pub fn update_tools_search_engine<B: TerminalBackend>(controller: &mut TerminalController<B>, engine: &str) {
    controller
        .state_mut()
        .update_modal_option_desc("Tools & Permissions", 0, engine);
}

fn toggle_tool_state_description<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    index: usize,
    default_on: bool,
) {
    let current = controller
        .state()
        .active_modal()
        .and_then(|m| m.options.get(index))
        .and_then(|o| o.description.as_deref())
        .unwrap_or(if default_on { "On" } else { "Off" });
    let next = if current == "On" { "Off" } else { "On" };
    update_tools_option_description(controller, next, index);
}

fn toggle_tools_setting<B: TerminalBackend>(controller: &mut TerminalController<B>, selected: usize) -> ModalKeyResult {
    match selected {
        0 => ModalKeyResult::OpenSearchEngineSelector,
        1 => {
            toggle_tool_state_description(controller, 1, false);
            let enabled = controller
                .state()
                .active_modal()
                .and_then(|m| m.options.get(1))
                .and_then(|o| o.description.as_deref())
                == Some("On");
            ModalKeyResult::WebSearchToggled { enabled }
        }
        2 => {
            toggle_tool_state_description(controller, 2, false);
            let enabled = controller
                .state()
                .active_modal()
                .and_then(|m| m.options.get(2))
                .and_then(|o| o.description.as_deref())
                == Some("On");
            ModalKeyResult::WebFetchToggled { enabled }
        }
        3 => {
            toggle_tool_state_description(controller, 3, false);
            let enabled = controller
                .state()
                .active_modal()
                .and_then(|m| m.options.get(3))
                .and_then(|o| o.description.as_deref())
                == Some("On");
            ModalKeyResult::McpToggled { enabled }
        }
        4 => {
            toggle_tool_state_description(controller, 4, true);
            ModalKeyResult::Handled
        }
        5 => {
            let current = controller
                .state()
                .active_modal()
                .and_then(|m| m.options.get(5))
                .and_then(|o| o.description.as_deref())
                .unwrap_or("Hashline");
            let next = if current == "Hashline" { "Exact" } else { "Hashline" };
            update_tools_option_description(controller, next, 5);
            ModalKeyResult::Handled
        }
        6 => {
            toggle_tool_state_description(controller, 6, false);
            let enabled = controller
                .state()
                .active_modal()
                .and_then(|m| m.options.get(6))
                .and_then(|o| o.description.as_deref())
                == Some("On");
            ModalKeyResult::PermissionToggled { enabled }
        }
        _ => ModalKeyResult::Handled,
    }
}

pub fn handle_tools_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Enter | KeyCode::Char(' ') => {
            let selected = controller.state().active_modal().map_or(0, |m| m.selected);
            let res = toggle_tools_setting(controller, selected);
            controller.redraw()?;
            Ok(res)
        }
        KeyCode::Esc => {
            crate::repl::live::modal::pop_and_cancel(controller)?;
            Ok(ModalKeyResult::Handled)
        }
        _ => {
            crate::repl::live::modal::handle_selector_nav(controller, &key)?;
            Ok(ModalKeyResult::Handled)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::interactive::{InteractiveState, TerminalBackend, TerminalController};
    use crossterm::event::{KeyCode, KeyModifiers};
    use std::io;

    struct DummyBackend;

    impl TerminalBackend for DummyBackend {
        fn set_raw_mode(&mut self, _enabled: bool) -> io::Result<()> {
            Ok(())
        }
        fn size(&self) -> io::Result<(u16, u16)> {
            Ok((80, 24))
        }
        fn hide_cursor(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn show_cursor(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn move_up(&mut self, _rows: usize) -> io::Result<()> {
            Ok(())
        }
        fn move_down(&mut self, _rows: usize) -> io::Result<()> {
            Ok(())
        }
        fn move_to_column(&mut self, _column: usize) -> io::Result<()> {
            Ok(())
        }
        fn clear_line(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn write_text(&mut self, _text: &str) -> io::Result<()> {
            Ok(())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn create_test_session() -> ReplSession {
        let temp = tempfile::tempdir().unwrap();
        let config = rho_harness_core::config::Config {
            config_dir: temp.path().to_path_buf(),
            sessions_dir: temp.path().join("sessions"),
            ..Default::default()
        };
        ReplSession::new(config, rho_engine::auth::AuthStore::default(), None)
    }

    #[test]
    fn build_tools_options_has_all_entries() {
        let session = create_test_session();
        let options = build_tools_options(&session);
        assert_eq!(options.len(), 7);
        assert_eq!(options[0].label, "Search Engine     ");
        assert_eq!(options[0].description.as_deref(), Some("brave"));
        assert_eq!(options[1].label, "Web Search        ");
        assert_eq!(options[1].description.as_deref(), Some("On"));
        assert_eq!(options[2].label, "Web Fetch         ");
        assert_eq!(options[2].description.as_deref(), Some("On"));
        assert_eq!(options[3].label, "MCP               ");
        assert_eq!(options[3].description.as_deref(), Some("On"));
        assert_eq!(options[4].label, "LSP Diagnostics   ");
        assert_eq!(options[4].description.as_deref(), Some("On"));
        assert_eq!(options[5].label, "Edit Mode         ");
        assert_eq!(options[5].description.as_deref(), Some("Hashline"));
        assert_eq!(options[6].label, "Permissions       ");
        assert_eq!(options[6].description.as_deref(), Some("On"));
    }

    #[test]
    fn open_tools_modal_and_toggle_options() {
        let session = create_test_session();
        let mut controller = TerminalController::new(DummyBackend, InteractiveState::default()).unwrap();

        open_tools_selector(&session, &mut controller);
        assert_eq!(controller.state().active_modal().unwrap().title, "Tools & Permissions");

        // Select 0: Search Engine -> OpenSearchEngineSelector
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let res = handle_tools_key(&mut controller, enter).unwrap();
        assert_eq!(res, ModalKeyResult::OpenSearchEngineSelector);

        // Move to 1: Web Search -> WebSearchToggled
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        let _ = handle_tools_key(&mut controller, down).unwrap();
        let space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
        let res = handle_tools_key(&mut controller, space).unwrap();
        assert_eq!(res, ModalKeyResult::WebSearchToggled { enabled: false });

        // Move to 2: Web Fetch -> WebFetchToggled
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        let _ = handle_tools_key(&mut controller, down).unwrap();
        let space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
        let res = handle_tools_key(&mut controller, space).unwrap();
        assert_eq!(res, ModalKeyResult::WebFetchToggled { enabled: false });

        // Move to 3: MCP -> McpToggled
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        let _ = handle_tools_key(&mut controller, down).unwrap();
        let space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
        let res = handle_tools_key(&mut controller, space).unwrap();
        assert_eq!(res, ModalKeyResult::McpToggled { enabled: false });

        // Move to 4: LSP Diagnostics (toggle Handled)
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        let _ = handle_tools_key(&mut controller, down).unwrap();
        let space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
        let res = handle_tools_key(&mut controller, space).unwrap();
        assert_eq!(res, ModalKeyResult::Handled);

        // Move to 5: Edit Mode (toggle Handled)
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        let _ = handle_tools_key(&mut controller, down).unwrap();
        let space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
        let res = handle_tools_key(&mut controller, space).unwrap();
        assert_eq!(res, ModalKeyResult::Handled);

        // Move to 6: Permissions -> PermissionToggled
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        let _ = handle_tools_key(&mut controller, down).unwrap();
        let space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
        let res = handle_tools_key(&mut controller, space).unwrap();
        assert_eq!(res, ModalKeyResult::PermissionToggled { enabled: false });

        // Esc pops modal
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        let res = handle_tools_key(&mut controller, esc).unwrap();
        assert_eq!(res, ModalKeyResult::Handled);
        assert!(controller.state().active_modal().is_none());
    }

    #[test]
    fn update_tools_search_engine_updates_description_in_active_modal() {
        let session = create_test_session();
        let mut controller = TerminalController::new(DummyBackend, InteractiveState::default()).unwrap();

        open_tools_selector(&session, &mut controller);
        assert_eq!(
            controller.state().active_modal().unwrap().options[0]
                .description
                .as_deref(),
            Some("brave")
        );

        update_tools_search_engine(&mut controller, "duckduckgo");
        assert_eq!(
            controller.state().active_modal().unwrap().options[0]
                .description
                .as_deref(),
            Some("duckduckgo")
        );
    }
}
