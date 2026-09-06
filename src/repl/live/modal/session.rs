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

fn selected_session_id<B: TerminalBackend>(controller: &TerminalController<B>) -> Option<String> {
    let opt = controller.state().active_modal().and_then(|m| m.selected_option())?;
    let desc = opt.description.clone().unwrap_or_default();
    Some(desc.split('\t').next().unwrap_or(&desc).trim().to_string())
}

fn pop_and_redraw<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    controller.state_mut().pop_modal();
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}

fn apply_session_filter<B: TerminalBackend>(
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

fn clear_session_filter<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<()> {
    let has_filter = controller
        .state()
        .active_modal()
        .is_some_and(|m| !m.filter_query.is_empty());
    if has_filter {
        if let Some(modal) = controller.state_mut().active_modal_mut() {
            modal.set_filter("");
        }
        controller.redraw()?;
        return Ok(());
    }
    pop_and_redraw(controller).map(|_| ())
}

fn delete_selected_session<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    let Some(session_id) = selected_session_id(controller) else {
        return Ok(ModalKeyResult::Handled);
    };
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        modal.all_options.retain(|o| {
            let opt_desc = o.description.as_deref().unwrap_or("");
            opt_desc.split('\t').next().unwrap_or(opt_desc).trim() != session_id
        });
        let q = modal.filter_query.clone();
        modal.set_filter(&q);
    }
    controller.redraw()?;
    Ok(ModalKeyResult::SessionDeleted { session_id })
}

fn handle_session_nav<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: &KeyEvent,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Backspace => apply_session_filter(controller, None),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            clear_session_filter(controller)?;
            Ok(ModalKeyResult::Handled)
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => delete_selected_session(controller),
        KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
            apply_session_filter(controller, Some(c))
        }
        _ => Ok(ModalKeyResult::Handled),
    }
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
            let Some(session_id) = selected_session_id(controller) else {
                return pop_and_redraw(controller);
            };
            pop_and_redraw(controller)?;
            Ok(ModalKeyResult::SessionSelected { session_id })
        }
        KeyCode::Esc => pop_and_redraw(controller),
        _ => handle_session_nav(controller, &key),
    }
}
