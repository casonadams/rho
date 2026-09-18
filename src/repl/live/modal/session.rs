use crate::error::Result;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use crate::ui::render::formatters::format_relative_time;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::Path;

use super::ModalKeyResult;

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

pub fn handle_session_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    if key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return delete_selected_session(controller);
    }
    super::dispatch_simple_selector(controller, key, |opt| {
        let desc = opt.description.as_deref().unwrap_or("");
        let session_id = desc.split('\t').next().unwrap_or(desc).trim().to_string();
        Some(ModalKeyResult::SessionSelected { session_id })
    })
}
