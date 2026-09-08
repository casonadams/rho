#[cfg(test)]
mod tests;

use crate::ui::interactive::{ModalOption, ModalState, TerminalController};
use crate::ui::render::formatters::format_relative_time;
use crate::ui::theme::Theme;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use rho_harness_core::error::Result;
use rho_harness_core::session::{SessionManager, SessionSummary};
use std::path::Path;

pub fn prompt_session_picker(sessions_dir: &Path, theme: &Theme) -> Result<Option<String>> {
    let summaries = SessionManager::list_session_summaries(sessions_dir)?;
    if summaries.is_empty() {
        return Ok(None);
    }

    let mut controller = TerminalController::stdout(crate::ui::interactive::InteractiveState::default())?;
    controller.set_theme(theme.clone())?;
    controller.state_mut().push_modal(session_modal(&summaries));
    controller.redraw()?;

    key_loop(&mut controller)
}

pub fn session_modal(summaries: &[SessionSummary]) -> ModalState {
    let options = summaries
        .iter()
        .map(|s| {
            let title = s.name.as_deref().unwrap_or(&s.preview);
            let time = format_relative_time(s.last_modified);
            let label = format!("{title} ({} | {} turns | {time})", s.session_id, s.turn_count);
            ModalOption::new(label, Some(s.session_id.clone()))
        })
        .collect();
    ModalState::new("Resume Session", "", options).with_search(true)
}

enum PickerAction {
    Repaint,
    Select(String),
    Cancel,
}

impl std::fmt::Debug for PickerAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Repaint => write!(f, "Repaint"),
            Self::Select(id) => write!(f, "Select({id})"),
            Self::Cancel => write!(f, "Cancel"),
        }
    }
}

fn handle_filter_key(modal: &mut ModalState, key: &KeyEvent) -> bool {
    match key.code {
        KeyCode::Backspace => {
            let mut query = modal.filter_query.clone();
            query.pop();
            modal.set_filter(&query);
            true
        }
        KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
            let mut query = modal.filter_query.clone();
            query.push(c);
            modal.set_filter(&query);
            true
        }
        _ => false,
    }
}

fn picker_enter(modal: &ModalState) -> PickerAction {
    let session_id = modal
        .selected_option()
        .and_then(|o| o.description.clone())
        .unwrap_or_default();
    PickerAction::Select(session_id)
}

fn picker_step(modal: &mut ModalState, prev: bool) -> PickerAction {
    if prev {
        modal.select_previous();
    } else {
        modal.select_next();
    }
    PickerAction::Repaint
}

fn picker_action(modal: &mut ModalState, key: &KeyEvent) -> PickerAction {
    match key.code {
        KeyCode::Up | KeyCode::BackTab => picker_step(modal, true),
        KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => picker_step(modal, true),
        KeyCode::Down | KeyCode::Tab => picker_step(modal, false),
        KeyCode::Enter => picker_enter(modal),
        KeyCode::Esc => PickerAction::Cancel,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => PickerAction::Cancel,
        _ => {
            handle_filter_key(modal, key);
            PickerAction::Repaint
        }
    }
}

fn key_loop(controller: &mut TerminalController<crate::ui::interactive::CrosstermBackend>) -> Result<Option<String>> {
    loop {
        let Event::Key(key) = crossterm::event::read()? else {
            continue;
        };
        if key.kind != crossterm::event::KeyEventKind::Press {
            continue;
        }
        let Some(modal) = controller.state_mut().active_modal_mut() else {
            return Ok(None);
        };
        match picker_action(modal, &key) {
            PickerAction::Repaint => controller.redraw()?,
            PickerAction::Select(session_id) => {
                controller.state_mut().pop_modal();
                controller.redraw()?;
                return Ok(Some(session_id));
            }
            PickerAction::Cancel => {
                controller.state_mut().pop_modal();
                controller.redraw()?;
                return Ok(None);
            }
        }
    }
}
