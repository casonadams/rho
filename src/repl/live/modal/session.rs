use crate::error::Result;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use chrono::{DateTime, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::Path;

use super::ModalKeyResult;

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

pub fn open_session_selector<B: TerminalBackend>(sessions_dir: &Path, controller: &mut TerminalController<B>) {
    let summaries = rho_harness_core::session::list_session_summaries(sessions_dir).unwrap_or_default();
    let mut options = Vec::new();

    for item in summaries {
        let display_title = item.name.unwrap_or_else(|| item.session_id.clone());
        let relative_time = format_relative_time(item.last_modified);
        let desc = format!("{}\t{} turns\t{}", item.session_id, item.turn_count, item.preview);
        let label = format!("{display_title} ({relative_time})");
        options.push(ModalOption::new(label, Some(desc)));
    }

    let modal = ModalState::new("Resume Session", "", options).with_search(true);
    controller.state_mut().push_modal(modal);
}

pub fn handle_session_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Up | KeyCode::BackTab => {
            controller.state_mut().select_previous_modal_option();
            controller.redraw()?;
            Ok(ModalKeyResult::Handled)
        }
        KeyCode::Down | KeyCode::Tab => {
            controller.state_mut().select_next_modal_option();
            controller.redraw()?;
            Ok(ModalKeyResult::Handled)
        }
        KeyCode::Enter => {
            if let Some(opt) = controller.state().active_modal().and_then(|m| m.selected_option()) {
                let desc = opt.description.clone().unwrap_or_default();
                let session_id = desc.split('\t').next().unwrap_or(&desc).trim().to_string();
                controller.state_mut().pop_modal();
                return Ok(ModalKeyResult::SessionSelected { session_id });
            }
            controller.state_mut().pop_modal();
            Ok(ModalKeyResult::Handled)
        }
        KeyCode::Esc => {
            controller.state_mut().pop_modal();
            controller.redraw()?;
            Ok(ModalKeyResult::Handled)
        }
        KeyCode::Backspace => {
            if let Some(modal) = controller.state_mut().active_modal_mut() {
                let mut query = modal.filter_query.clone();
                query.pop();
                modal.set_filter(&query);
            }
            controller.redraw()?;
            Ok(ModalKeyResult::Handled)
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            controller.state_mut().pop_modal();
            controller.redraw()?;
            Ok(ModalKeyResult::Handled)
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(opt) = controller.state().active_modal().and_then(|m| m.selected_option()) {
                let desc = opt.description.clone().unwrap_or_default();
                let session_id = desc.split('\t').next().unwrap_or(&desc).trim().to_string();
                if let Some(modal) = controller.state_mut().active_modal_mut() {
                    modal.all_options.retain(|o| {
                        let opt_desc = o.description.as_deref().unwrap_or("");
                        opt_desc.split('\t').next().unwrap_or(opt_desc).trim() != session_id
                    });
                    let q = modal.filter_query.clone();
                    modal.set_filter(&q);
                }
                controller.redraw()?;
                return Ok(ModalKeyResult::SessionDeleted { session_id });
            }
            Ok(ModalKeyResult::Handled)
        }
        KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
            if let Some(modal) = controller.state_mut().active_modal_mut() {
                let mut query = modal.filter_query.clone();
                query.push(c);
                modal.set_filter(&query);
            }
            controller.redraw()?;
            Ok(ModalKeyResult::Handled)
        }
        _ => Ok(ModalKeyResult::Handled),
    }
}
