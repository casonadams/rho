use crate::error::Result;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent};

use super::ModalKeyResult;

pub fn open_settings_selector<B: TerminalBackend>(controller: &mut TerminalController<B>) {
    let hide_thinking = controller.state().hide_thinking();
    let tools_expanded = controller.state().tools_expanded();

    let thinking_status = if hide_thinking { "Hidden" } else { "Shown" };
    let tools_status = if tools_expanded { "Expanded" } else { "Collapsed" };

    let options = vec![
        ModalOption::new(
            "Thinking Blocks",
            Some(format!("{thinking_status}  (press Enter to toggle)")),
        ),
        ModalOption::new("Tool Output", Some(format!("{tools_status}  (press Enter to toggle)"))),
    ];

    let modal = ModalState::new("Settings", "Toggle runtime interface settings:", options);
    controller.state_mut().push_modal(modal);
}

pub fn handle_settings_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Up | KeyCode::BackTab | KeyCode::Char('k') => {
            controller.state_mut().select_previous_modal_option();
            controller.redraw()?;
            Ok(ModalKeyResult::Handled)
        }
        KeyCode::Down | KeyCode::Tab | KeyCode::Char('j') => {
            controller.state_mut().select_next_modal_option();
            controller.redraw()?;
            Ok(ModalKeyResult::Handled)
        }
        KeyCode::Enter => {
            let selected = controller.state().active_modal().map_or(0, |m| m.selected);
            if selected == 0 {
                let hide = controller.state_mut().toggle_thinking();
                let status = if hide { "Hidden" } else { "Shown" };
                if let Some(modal) = controller.state_mut().active_modal_mut()
                    && let Some(opt) = modal.options.get_mut(0)
                {
                    opt.description = Some(format!("{status}  (press Enter to toggle)"));
                }
            } else if selected == 1 {
                let expanded = controller.state_mut().toggle_tools_expanded();
                let status = if expanded { "Expanded" } else { "Collapsed" };
                if let Some(modal) = controller.state_mut().active_modal_mut()
                    && let Some(opt) = modal.options.get_mut(1)
                {
                    opt.description = Some(format!("{status}  (press Enter to toggle)"));
                }
            }
            controller.redraw()?;
            Ok(ModalKeyResult::Handled)
        }
        KeyCode::Esc => {
            controller.state_mut().pop_modal();
            controller.redraw()?;
            Ok(ModalKeyResult::Handled)
        }
        _ => Ok(ModalKeyResult::Handled),
    }
}
