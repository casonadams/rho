#[cfg(test)]
mod tests;

use crate::ui::interactive::{ModalOption, ModalState, TerminalController};
use crate::ui::theme::Theme;
use chrono::{DateTime, Utc};
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

/// Runs inside the live UI's Resume Session modal too; shared for identical labels.
pub fn format_relative_time(time: DateTime<Utc>) -> String {
    let now = Utc::now();
    let diff = now.signed_duration_since(time);
    let secs = diff.num_seconds();
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else if secs < 2592000 {
        format!("{}d ago", secs / 86400)
    } else {
        time.format("%Y-%m-%d").to_string()
    }
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

fn picker_action(modal: &mut ModalState, key: &KeyEvent) -> PickerAction {
    match key.code {
        KeyCode::Up | KeyCode::BackTab => {
            modal.select_previous();
            PickerAction::Repaint
        }
        KeyCode::Down | KeyCode::Tab => {
            modal.select_next();
            PickerAction::Repaint
        }
        KeyCode::Enter => {
            let session_id = modal
                .selected_option()
                .and_then(|o| o.description.clone())
                .unwrap_or_default();
            PickerAction::Select(session_id)
        }
        KeyCode::Esc => PickerAction::Cancel,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => PickerAction::Cancel,
        KeyCode::Backspace => {
            let mut query = modal.filter_query.clone();
            query.pop();
            modal.set_filter(&query);
            PickerAction::Repaint
        }
        KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
            let mut query = modal.filter_query.clone();
            query.push(c);
            modal.set_filter(&query);
            PickerAction::Repaint
        }
        _ => PickerAction::Repaint,
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
