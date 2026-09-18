use crate::error::Result;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use crossterm::event::KeyEvent;

use super::ModalKeyResult;

fn build_help_options() -> Vec<ModalOption> {
    vec![
        ModalOption::new(
            format!("{:<15}", "/settings"),
            Some("Interactive runtime interface settings"),
        ),
        ModalOption::new(format!("{:<15}", "/model"), Some("Inspect or switch the active model")),
        ModalOption::new(format!("{:<15}", "/resume"), Some("Resume a prior session")),
        ModalOption::new(
            format!("{:<15}", "/session"),
            Some("Token capacity & diagnostics (alias: /tokens)"),
        ),
        ModalOption::new(
            format!("{:<15}", "/compact"),
            Some("Summarize earlier context to free space"),
        ),
        ModalOption::new(
            format!("{:<15}", "/tree"),
            Some("View conversation turn and branch tree"),
        ),
        ModalOption::new(
            format!("{:<15}", "/fork"),
            Some("Fork session from turn into a new session"),
        ),
        ModalOption::new(
            format!("{:<15}", "/clone"),
            Some("Duplicate active branch into a new session"),
        ),
        ModalOption::new(
            format!("{:<15}", "/name"),
            Some("Assign a human-readable name to session"),
        ),
        ModalOption::new(
            format!("{:<15}", "/rewind"),
            Some("Rewind context to a specific prior turn"),
        ),
        ModalOption::new(
            format!("{:<15}", "/clear"),
            Some("Start a new session; preserve history (alias: /new)"),
        ),
        ModalOption::new(format!("{:<15}", "/mcp"), Some("List configured MCP servers")),
        ModalOption::new(format!("{:<15}", "/login"), Some("Add API-key or subscription auth")),
        ModalOption::new(format!("{:<15}", "/logout"), Some("Remove stored provider auth")),
        ModalOption::new(format!("{:<15}", "/skill"), Some("List or inspect skills")),
        ModalOption::new(
            format!("{:<15}", "/reload"),
            Some("Re-read config, skills, and MCP tools"),
        ),
        ModalOption::new(
            format!("{:<15}", "/export"),
            Some("Export active branch as HTML or Markdown"),
        ),
        ModalOption::new(
            format!("{:<15}", "/remote"),
            Some("Pair session with web dashboard via Iroh P2P"),
        ),
        ModalOption::new(format!("{:<15}", "/exit"), Some("Exit rho (alias: /quit)")),
        ModalOption::new(format!("{:<15}", "Tab"), Some("Complete slash commands & skill names")),
        ModalOption::new(format!("{:<15}", "Shift+Tab"), Some("Cycle thinking level")),
        ModalOption::new(format!("{:<15}", "Ctrl+L"), Some("Select model")),
        ModalOption::new(format!("{:<15}", "Ctrl+O"), Some("Expand or collapse tool output")),
        ModalOption::new(format!("{:<15}", "Ctrl+T"), Some("Toggle thinking blocks visibility")),
        ModalOption::new(format!("{:<15}", "Ctrl+C"), Some("Clear the input prompt")),
        ModalOption::new(format!("{:<15}", "Ctrl+D"), Some("Exit at an empty prompt")),
        ModalOption::new(
            format!("{:<15}", "Escape"),
            Some("Cancel active execution or dismiss modal"),
        ),
    ]
}

pub fn open_help_selector<B: TerminalBackend>(controller: &mut TerminalController<B>) {
    let options = build_help_options();
    let modal = ModalState::new("Help", "", options).with_search(true);
    controller.state_mut().push_modal(modal);
}

pub fn handle_help_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    super::dispatch_simple_selector(controller, key, |opt| {
        let raw = opt.label.trim();
        raw.starts_with('/').then(|| ModalKeyResult::HelpCommandSelected {
            command: raw.to_string(),
        })
    })
}
