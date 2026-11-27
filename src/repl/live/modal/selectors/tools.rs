use crossterm::event::{KeyCode, KeyEvent};

use super::super::{ModalKeyResult, handle_selector_key};
use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalKind, ModalOption, ModalState, TerminalBackend, TerminalController};

const ON: &str = "On";
const OFF: &str = "Off";

/// Rows of the tools modal in display order; a row's position is its option index.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
    SearchEngine,
    WebSearch,
    WebFetch,
    Mcp,
    Permissions,
}

impl Row {
    const ALL: [Row; 5] = [
        Row::SearchEngine,
        Row::WebSearch,
        Row::WebFetch,
        Row::Mcp,
        Row::Permissions,
    ];

    fn index(self) -> usize {
        Self::ALL.iter().position(|row| *row == self).unwrap_or_default()
    }

    fn from_index(index: usize) -> Option<Row> {
        Self::ALL.get(index).copied()
    }

    fn label(self) -> &'static str {
        match self {
            Row::SearchEngine => "Search Engine     ",
            Row::WebSearch => "Web Search        ",
            Row::WebFetch => "Web Fetch         ",
            Row::Mcp => "MCP               ",
            Row::Permissions => "Permissions       ",
        }
    }

    fn status(self, session: &ReplSession) -> String {
        let config = &session.config;
        match self {
            Row::SearchEngine => config.tools.web.search.default.to_string(),
            Row::WebSearch => on_off(config.tools.web.search.enabled).to_string(),
            Row::WebFetch => on_off(config.tools.web.fetch.enabled).to_string(),
            Row::Mcp => on_off(config.mcp.enabled).to_string(),
            Row::Permissions => on_off(config.permission.enabled).to_string(),
        }
    }
}

fn on_off(on: bool) -> &'static str {
    if on { ON } else { OFF }
}

pub fn build_tools_options(session: &ReplSession) -> Vec<ModalOption> {
    Row::ALL
        .iter()
        .map(|row| ModalOption::new(row.label(), Some(row.status(session))))
        .collect()
}

pub fn open_tools_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    let options = build_tools_options(session);
    let modal = ModalState::new("Tools & Permissions", "", options).with_kind(ModalKind::Tools);
    controller.state_mut().push_modal(modal);
}

pub fn update_tools_search_engine<B: TerminalBackend>(controller: &mut TerminalController<B>, engine: &str) {
    controller
        .state_mut()
        .update_modal_option_desc(&ModalKind::Tools, Row::SearchEngine.index(), engine);
}

/// Flips the on/off status of `row` and returns the new state.
fn toggle_row<B: TerminalBackend>(controller: &mut TerminalController<B>, row: Row) -> bool {
    let enabled = controller
        .state()
        .active_modal()
        .and_then(|m| m.options.get(row.index()))
        .and_then(|o| o.description.as_deref())
        == Some(ON);
    let next = !enabled;
    controller
        .state_mut()
        .update_modal_option_desc(&ModalKind::Tools, row.index(), on_off(next));
    next
}

fn activate_row<B: TerminalBackend>(controller: &mut TerminalController<B>, row: Row) -> ModalKeyResult {
    match row {
        Row::SearchEngine => ModalKeyResult::OpenSearchEngineSelector,
        Row::WebSearch => ModalKeyResult::WebSearchToggled {
            enabled: toggle_row(controller, row),
        },
        Row::WebFetch => ModalKeyResult::WebFetchToggled {
            enabled: toggle_row(controller, row),
        },
        Row::Mcp => ModalKeyResult::McpToggled {
            enabled: toggle_row(controller, row),
        },
        Row::Permissions => ModalKeyResult::PermissionToggled {
            enabled: toggle_row(controller, row),
        },
    }
}

fn activate_selected<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    let selected = controller.state().active_modal().map_or(0, |m| m.selected);
    let res = Row::from_index(selected).map_or(ModalKeyResult::Handled, |row| activate_row(controller, row));
    controller.redraw()?;
    Ok(res)
}

pub fn handle_tools_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    if key.code == KeyCode::Char(' ') {
        return activate_selected(controller);
    }
    handle_selector_key(controller, key, activate_selected)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::create_test_session;
    use super::*;
    use crate::repl::test_controller;
    use crossterm::event::KeyModifiers;

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
        let mut controller = test_controller();

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
        let mut controller = test_controller();

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
