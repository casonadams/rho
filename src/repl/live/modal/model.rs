use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::ModalKeyResult;

pub fn open_model_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    let discovered = crate::repl::interactive::discover_models(&session.config, &session.auth_store);
    let mut options = Vec::new();
    let mut initial_selection = 0;

    for (i, item) in discovered.iter().enumerate() {
        let is_active = item.id == session.config.model;
        if is_active {
            initial_selection = i;
        }
        let active_mark = if is_active { "✓" } else { "" };
        let default_mark = if is_active { "default" } else { "" };
        options.push(ModalOption::new(
            item.id.clone(),
            Some(format!(
                "{}\t{}\t{}\t{}",
                item.provider, active_mark, default_mark, item.description
            )),
        ));
    }

    let mut modal = ModalState::new("Select Model", "", options).with_search(true);
    modal.selected = initial_selection;
    controller.state_mut().push_modal(modal);
}

fn extract_selected_model<B: TerminalBackend>(controller: &TerminalController<B>) -> Option<(String, String)> {
    let opt = controller.state().active_modal().and_then(|m| m.selected_option())?;
    let selected_model = opt.label.clone();
    let provider = opt
        .description
        .as_deref()
        .and_then(|d| d.split('\t').next())
        .unwrap_or("anthropic")
        .to_string();
    Some((selected_model, provider))
}

pub fn handle_model_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if let Some((model, provider)) = extract_selected_model(controller) {
            controller.state_mut().pop_modal();
            return Ok(ModalKeyResult::ModelSelected {
                model,
                provider,
                save_as_default: true,
            });
        }
        controller.state_mut().pop_modal();
        return Ok(ModalKeyResult::Handled);
    }

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
            if let Some((model, provider)) = extract_selected_model(controller) {
                controller.state_mut().pop_modal();
                return Ok(ModalKeyResult::ModelSelected {
                    model,
                    provider,
                    save_as_default: false,
                });
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
