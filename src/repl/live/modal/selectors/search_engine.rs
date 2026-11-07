use crossterm::event::KeyEvent;

use super::super::ModalKeyResult;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};

const ENGINE_DEFS: &[(&str, &str)] = &[
    ("brave", "Brave Search (Free scraper, no API key)"),
    ("duckduckgo", "DuckDuckGo Lite (Free scraper, no API key)"),
    ("yahoo", "Yahoo Search (Free scraper, no API key)"),
];

fn format_engine_option(id: &str, desc: &str, active_engine: &str) -> ModalOption {
    let active = id.eq_ignore_ascii_case(active_engine);
    let active_mark = if active { "  ✓" } else { "" };
    ModalOption::new(format!("{id:14}"), Some(format!("{desc}{active_mark}")))
}

pub fn open_search_engine_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    let active_engine = &session.config.tools.web.search.default;
    let mut options = Vec::with_capacity(ENGINE_DEFS.len());
    let mut initial_selection = 0;

    for (i, &(id, desc)) in ENGINE_DEFS.iter().enumerate() {
        if id.eq_ignore_ascii_case(active_engine) {
            initial_selection = i;
        }
        options.push(format_engine_option(id, desc, active_engine));
    }

    let mut modal = ModalState::new("Select Search Engine", "", options).with_search(false);
    modal.selected = initial_selection;
    controller.state_mut().push_modal(modal);
}

pub fn handle_search_engine_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    crate::repl::live::modal::dispatch_simple_selector(controller, key, |opt| {
        Some(ModalKeyResult::SearchEngineSelected {
            engine: opt.label.trim().to_string(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{DummyBackend, assert_selector_opens_and_selects, create_test_session};
    use super::*;
    use crate::ui::interactive::{InteractiveState, TerminalController};
    use crossterm::event::{KeyCode, KeyModifiers};

    #[test]
    fn format_engine_option_aligns_and_marks_active() {
        let opt_active = format_engine_option("brave", "Brave Search", "brave");
        assert_eq!(opt_active.label, "brave         ");
        assert_eq!(opt_active.description.as_deref(), Some("Brave Search  ✓"));

        let opt_inactive = format_engine_option("duckduckgo", "DuckDuckGo Lite", "brave");
        assert_eq!(opt_inactive.label, "duckduckgo    ");
        assert_eq!(opt_inactive.description.as_deref(), Some("DuckDuckGo Lite"));
    }

    #[test]
    fn open_and_select_search_engine_modal() {
        assert_selector_opens_and_selects(
            open_search_engine_selector,
            handle_search_engine_key,
            "Select Search Engine",
            3,
            ModalKeyResult::SearchEngineSelected {
                engine: "brave".to_string(),
            },
            true,
        );
    }

    #[test]
    fn search_engine_selector_handles_esc() {
        let session = create_test_session();
        let mut controller = TerminalController::new(DummyBackend, InteractiveState::default()).unwrap();

        open_search_engine_selector(&session, &mut controller);
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        let res = handle_search_engine_key(&mut controller, key).unwrap();
        assert_eq!(res, ModalKeyResult::Handled);
        assert!(controller.state().active_modal().is_none());
    }
}
