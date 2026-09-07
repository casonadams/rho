use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::ModalKeyResult;

fn is_active_level(session: &ReplSession, level: &str) -> bool {
    match session.config.thinking_level.as_deref() {
        None | Some("off") => level == "off",
        Some(active) => level.eq_ignore_ascii_case(active),
    }
}

fn build_thinking_options(session: &ReplSession) -> (Vec<ModalOption>, usize) {
    let mut options = Vec::new();
    let mut initial_selection = 0;

    for (i, &(level, desc)) in crate::repl::interactive::completion::THINKING_LEVELS.iter().enumerate() {
        let active = is_active_level(session, level);
        if active {
            initial_selection = i;
        }
        let active_mark = if active { "  ✓" } else { "" };
        options.push(ModalOption::new(
            format!("{level:8}"),
            Some(format!("{desc}{active_mark}")),
        ));
    }

    (options, initial_selection)
}

pub fn open_thinking_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    let (options, initial_selection) = build_thinking_options(session);
    let mut modal = ModalState::new("Select Thinking Level", "", options);
    modal.selected = initial_selection;
    controller.state_mut().push_modal(modal);
}

fn extract_selected_level<B: TerminalBackend>(controller: &TerminalController<B>) -> Option<Option<String>> {
    let opt = controller.state().active_modal().and_then(|m| m.selected_option())?;
    let raw = opt.label.trim();
    if raw.eq_ignore_ascii_case("off") {
        Some(None)
    } else {
        Some(Some(raw.to_string()))
    }
}

fn pop_and_select<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    save_as_default: bool,
) -> Result<ModalKeyResult> {
    let selected = extract_selected_level(controller);
    controller.state_mut().pop_modal();
    controller.redraw()?;
    Ok(match selected {
        Some(level) => ModalKeyResult::ThinkingLevelSelected { level, save_as_default },
        None => ModalKeyResult::Handled,
    })
}

fn pop_and_cancel<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    controller.state_mut().pop_modal();
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}

fn handle_digit_jump<B: TerminalBackend>(controller: &mut TerminalController<B>, c: char) {
    let idx = (c as usize).saturating_sub('1' as usize);
    let count = controller.state().active_modal().map_or(0, |m| m.options.len());
    if idx < count {
        if let Some(modal) = controller.state_mut().active_modal_mut() {
            modal.selected = idx;
        }
        let _ = controller.redraw();
    }
}

fn handle_arrow_nav<B: TerminalBackend>(controller: &mut TerminalController<B>, key: &KeyEvent) -> bool {
    match key.code {
        KeyCode::Up | KeyCode::BackTab | KeyCode::Char('k') => {
            controller.state_mut().select_previous_modal_option();
            let _ = controller.redraw();
            true
        }
        KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
            controller.state_mut().select_previous_modal_option();
            let _ = controller.redraw();
            true
        }
        KeyCode::Down | KeyCode::Tab | KeyCode::Char('j') => {
            controller.state_mut().select_next_modal_option();
            let _ = controller.redraw();
            true
        }
        _ => false,
    }
}

fn handle_nav_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: &KeyEvent,
) -> Result<ModalKeyResult> {
    if !handle_arrow_nav(controller, key)
        && let KeyCode::Char(c) = key.code
        && c.is_ascii_digit()
        && c != '0'
    {
        handle_digit_jump(controller, c);
    }
    Ok(ModalKeyResult::Handled)
}

pub fn handle_thinking_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Esc => pop_and_cancel(controller),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => pop_and_cancel(controller),
        KeyCode::Enter => pop_and_select(controller, false),
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => pop_and_select(controller, true),
        _ => handle_nav_key(controller, &key),
    }
}
