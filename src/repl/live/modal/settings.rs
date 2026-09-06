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

fn toggle_selected_setting<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    selected: usize,
) -> (&'static str, usize) {
    if selected == 0 {
        let hide = controller.state_mut().toggle_thinking();
        ((if hide { "Hidden" } else { "Shown" }), 0)
    } else {
        let expanded = controller.state_mut().toggle_tools_expanded();
        ((if expanded { "Expanded" } else { "Collapsed" }), 1)
    }
}

fn update_setting_description<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    (status, index): (&str, usize),
) {
    if let Some(modal) = controller.state_mut().active_modal_mut()
        && let Some(opt) = modal.options.get_mut(index)
    {
        opt.description = Some(format!("{status}  (press Enter to toggle)"));
    }
}

fn pop_and_redraw_settings<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    controller.state_mut().pop_modal();
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}

fn is_settings_exit(key: &KeyEvent) -> bool {
    key.code == KeyCode::Esc
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL))
}

fn handle_settings_nav<B: TerminalBackend>(controller: &mut TerminalController<B>, key: &KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Up | KeyCode::BackTab | KeyCode::Char('k') => {
            controller.state_mut().select_previous_modal_option();
        }
        KeyCode::Down | KeyCode::Tab | KeyCode::Char('j') => {
            controller.state_mut().select_next_modal_option();
        }
        KeyCode::Enter => {
            let selected = controller.state().active_modal().map_or(0, |m| m.selected);
            let (status, index) = toggle_selected_setting(controller, selected);
            update_setting_description(controller, (status, index));
        }
        _ => {}
    }
    controller.redraw()?;
    Ok(())
}

pub fn handle_settings_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    if is_settings_exit(&key) {
        return pop_and_redraw_settings(controller);
    }
    handle_settings_nav(controller, &key)?;
    Ok(ModalKeyResult::Handled)
}
