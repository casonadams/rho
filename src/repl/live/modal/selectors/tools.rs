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

fn toggle_tool_state_description<B: TerminalBackend>(controller: &mut TerminalController<B>, index: usize) {
    let current = controller
        .state()
        .active_modal()
        .and_then(|m| m.options.get(index))
        .and_then(|o| o.description.as_deref())
        .unwrap_or("Off");
    let next = if current == "On" { "Off" } else { "On" };
    update_tools_option_description(controller, next, index);
}

fn toggle_tools_setting<B: TerminalBackend>(controller: &mut TerminalController<B>, selected: usize) -> ModalKeyResult {
    match selected {
        0 => ModalKeyResult::OpenSearchEngineSelector,
        1 => {
            toggle_tool_state_description(controller, 1);
            let enabled = controller
                .state()
                .active_modal()
                .and_then(|m| m.options.get(1))
                .and_then(|o| o.description.as_deref())
                == Some("On");
            ModalKeyResult::WebSearchToggled { enabled }
        }
        2 => {
            toggle_tool_state_description(controller, 2);
            let enabled = controller
                .state()
                .active_modal()
                .and_then(|m| m.options.get(2))
                .and_then(|o| o.description.as_deref())
                == Some("On");
            ModalKeyResult::WebFetchToggled { enabled }
        }
        3 => {
            toggle_tool_state_description(controller, 3);
            let enabled = controller
                .state()
                .active_modal()
                .and_then(|m| m.options.get(3))
                .and_then(|o| o.description.as_deref())
                == Some("On");
            ModalKeyResult::McpToggled { enabled }
        }
        4 => {
            toggle_tool_state_description(controller, 4);
            let enabled = controller
                .state()
                .active_modal()
                .and_then(|m| m.options.get(4))
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
    use super::super::test_support::{DummyBackend, create_test_session};
    use super::*;
    use crate::ui::interactive::{InteractiveState, TerminalController};
    use crossterm::event::{KeyCode, KeyModifiers};

    #[test]
    fn build_tools_options_has_all_entries() {
        let session = create_test_session();
        let options = build_tools_options(&session);
        assert_eq!(options.len(), 5);
        assert_eq!(options[0].label, "Search Engine     ");
        assert_eq!(options[0].description.as_deref(), Some("brave"));
        assert_eq!(options[1].label, "Web Search        ");
        assert_eq!(options[1].description.as_deref(), Some("On"));
        assert_eq!(options[2].label, "Web Fetch         ");
        assert_eq!(options[2].description.as_deref(), Some("On"));
        assert_eq!(options[3].label, "MCP               ");
        assert_eq!(options[3].description.as_deref(), Some("On"));
        assert_eq!(options[4].label, "Permissions       ");
        assert_eq!(options[4].description.as_deref(), Some("On"));
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

        // Move to 4: Permissions -> PermissionToggled
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
