use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent};

use super::ModalKeyResult;

pub fn open_theme_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    let registry = crate::ui::theme::ThemeRegistry::new(Some(&session.config.config_dir));
    let themes = registry.list();
    let mut options = Vec::new();
    let mut initial_selection = 0;

    for (i, item) in themes.iter().enumerate() {
        let is_active = item.name == session.config.theme
            || (session.config.theme == "default" && item.name == "ansi")
            || (session.config.theme == "ansi" && item.name == "default");
        if is_active {
            initial_selection = i;
        }
        let active_mark = if is_active { "✓" } else { "" };
        let kind = if item.is_light {
            "light"
        } else if item.is_custom {
            "custom"
        } else {
            "dark"
        };
        let default_mark = if is_active { "active" } else { "" };
        options.push(ModalOption::new(
            item.name.clone(),
            Some(format!("{kind}\t{active_mark}\t{default_mark}\t{}", item.description)),
        ));
    }

    let mut modal = ModalState::new("Select Theme", "", options).with_search(true);
    modal.selected = initial_selection;
    controller.state_mut().push_modal(modal);
}

fn extract_selected_theme<B: TerminalBackend>(controller: &TerminalController<B>) -> Option<String> {
    let opt = controller.state().active_modal().and_then(|m| m.selected_option())?;
    Some(opt.label.clone())
}

pub fn handle_theme_key<B: TerminalBackend>(
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
            if let Some(theme) = extract_selected_theme(controller) {
                controller.state_mut().pop_modal();
                return Ok(ModalKeyResult::ThemeSelected { theme });
            }
            controller.state_mut().pop_modal();
            controller.redraw()?;
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
        KeyCode::Char('c') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
            controller.state_mut().pop_modal();
            controller.redraw()?;
            Ok(ModalKeyResult::Handled)
        }
        KeyCode::Char(c)
            if !key
                .modifiers
                .intersects(crossterm::event::KeyModifiers::CONTROL | crossterm::event::KeyModifiers::ALT) =>
        {
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
