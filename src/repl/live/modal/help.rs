use crate::error::Result;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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

fn extract_selected_command<B: TerminalBackend>(controller: &TerminalController<B>) -> Option<String> {
    let opt = controller.state().active_modal().and_then(|m| m.selected_option())?;
    let raw = opt.label.trim();
    if raw.starts_with('/') {
        Some(raw.to_string())
    } else {
        None
    }
}

fn pop_and_select<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    let selected = extract_selected_command(controller);
    controller.state_mut().pop_modal();
    controller.redraw()?;
    Ok(match selected {
        Some(command) => ModalKeyResult::HelpCommandSelected { command },
        None => ModalKeyResult::Handled,
    })
}

fn pop_and_cancel<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    controller.state_mut().pop_modal();
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}

fn apply_help_filter<B: TerminalBackend>(
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
        KeyCode::Backspace => return apply_help_filter(controller, None),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return clear_filter_or_cancel(controller);
        }
        KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
            return apply_help_filter(controller, Some(c));
        }
        _ => {}
    }
    Ok(ModalKeyResult::Handled)
}

pub fn handle_help_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Enter => pop_and_select(controller),
        KeyCode::Esc => pop_and_cancel(controller),
        _ => handle_nav_or_filter(controller, &key),
    }
}
