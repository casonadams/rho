use crate::error::Result;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::ModalKeyResult;

const THINKING_LEVELS: &[&str] = &["off", "minimal", "low", "medium", "high", "xhigh", "max"];

fn next_thinking_level(current: &str) -> &'static str {
    let idx = THINKING_LEVELS
        .iter()
        .position(|&l| l.eq_ignore_ascii_case(current))
        .unwrap_or(0);
    let next_idx = (idx + 1) % THINKING_LEVELS.len();
    THINKING_LEVELS[next_idx]
}

fn prev_thinking_level(current: &str) -> &'static str {
    let idx = THINKING_LEVELS
        .iter()
        .position(|&l| l.eq_ignore_ascii_case(current))
        .unwrap_or(0);
    let prev_idx = if idx == 0 { THINKING_LEVELS.len() - 1 } else { idx - 1 };
    THINKING_LEVELS[prev_idx]
}

pub fn open_settings_selector<B: TerminalBackend>(
    model: Option<&str>,
    thinking_level: Option<&str>,
    controller: &mut TerminalController<B>,
) {
    let hide_thinking = controller.state().hide_thinking();
    let tools_expanded = controller.state().tools_expanded();
    let boxed = controller.block_agent_output();
    let show_label = controller.state().show_label();
    let block_style = match controller.block_style() {
        crate::ui::theme::BlockStyle::Border => "Border",
        crate::ui::theme::BlockStyle::Solid => "Solid",
    };

    let model_name = model.unwrap_or("default");
    let thinking_effort = thinking_level.unwrap_or("off");
    let thinking_status = if hide_thinking { "Hidden" } else { "Shown" };
    let tools_status = if tools_expanded { "Expanded" } else { "Collapsed" };
    let agent_status = if boxed { "On" } else { "Off" };
    let label_status = if show_label { "Shown" } else { "Hidden" };

    let options = vec![
        ModalOption::new("Block Style       ", Some(block_style.to_string())),
        ModalOption::new("Box Responses     ", Some(agent_status.to_string())),
        ModalOption::new("Model             ", Some(model_name.to_string())),
        ModalOption::new("Thinking Effort   ", Some(thinking_effort.to_string())),
        ModalOption::new("Thinking Output   ", Some(thinking_status.to_string())),
        ModalOption::new("Tool Output       ", Some(tools_status.to_string())),
        ModalOption::new("Version Banner    ", Some(label_status.to_string())),
    ];

    let modal = ModalState::new("Settings", "", options);
    controller.state_mut().push_modal(modal);
}

fn update_setting_description<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    (status, index): (&str, usize),
) {
    if let Some(modal) = controller.state_mut().active_modal_mut()
        && let Some(opt) = modal.options.get_mut(index)
    {
        opt.description = Some(status.to_string());
    }
}

fn current_thinking_from_modal<B: TerminalBackend>(controller: &TerminalController<B>) -> &str {
    controller
        .state()
        .active_modal()
        .and_then(|m| m.options.get(3))
        .and_then(|o| o.description.as_deref())
        .unwrap_or("off")
}

fn step_thinking_effort<B: TerminalBackend>(controller: &mut TerminalController<B>, forward: bool) -> ModalKeyResult {
    let current = current_thinking_from_modal(controller);
    let target = if forward {
        next_thinking_level(current)
    } else {
        prev_thinking_level(current)
    };
    update_setting_description(controller, (target, 3));
    ModalKeyResult::ThinkingLevelSelected {
        level: if target == "off" {
            None
        } else {
            Some(target.to_string())
        },
        save_as_default: false,
    }
}

fn toggle_selected_setting<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    selected: usize,
) -> ModalKeyResult {
    match selected {
        0 => {
            let next = controller
                .toggle_block_style()
                .unwrap_or(crate::ui::theme::BlockStyle::Solid);
            let label = match next {
                crate::ui::theme::BlockStyle::Border => "Border",
                crate::ui::theme::BlockStyle::Solid => "Solid",
            };
            update_setting_description(controller, (label, 0));
            ModalKeyResult::BlockStyleToggled {
                style: label.to_lowercase(),
            }
        }
        1 => {
            let boxed = controller.toggle_block_agent_output().unwrap_or(false);
            update_setting_description(controller, (if boxed { "On" } else { "Off" }, 1));
            ModalKeyResult::AgentBoxToggled { boxed }
        }
        2 => {
            controller.state_mut().pop_modal();
            let _ = controller.redraw();
            ModalKeyResult::OpenModelSelector
        }
        3 => step_thinking_effort(controller, true),
        4 => {
            let hide = controller.state_mut().toggle_thinking();
            update_setting_description(controller, (if hide { "Hidden" } else { "Shown" }, 4));
            ModalKeyResult::Handled
        }
        5 => {
            let expanded = controller.state_mut().toggle_tools_expanded();
            update_setting_description(controller, (if expanded { "Expanded" } else { "Collapsed" }, 5));
            ModalKeyResult::Handled
        }
        6 => {
            let shown = controller.state_mut().toggle_show_label();
            update_setting_description(controller, (if shown { "Shown" } else { "Hidden" }, 6));
            ModalKeyResult::ShowLabelToggled { shown }
        }
        _ => ModalKeyResult::Handled,
    }
}

fn pop_and_redraw_settings<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    controller.state_mut().pop_modal();
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}

fn is_settings_exit(key: &KeyEvent) -> bool {
    key.code == KeyCode::Esc || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
}

fn handle_digit_jump<B: TerminalBackend>(controller: &mut TerminalController<B>, c: char) {
    let idx = (c as usize).saturating_sub('1' as usize);
    let count = controller.state().active_modal().map_or(0, |m| m.options.len());
    if idx < count
        && let Some(modal) = controller.state_mut().active_modal_mut()
    {
        modal.selected = idx;
    }
}

fn handle_arrow_nav<B: TerminalBackend>(controller: &mut TerminalController<B>, key: &KeyEvent) -> bool {
    match key.code {
        KeyCode::Up | KeyCode::BackTab | KeyCode::Char('k') => {
            controller.state_mut().select_previous_modal_option();
            true
        }
        KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
            controller.state_mut().select_previous_modal_option();
            true
        }
        KeyCode::Down | KeyCode::Tab | KeyCode::Char('j') => {
            controller.state_mut().select_next_modal_option();
            true
        }
        _ => false,
    }
}

fn handle_settings_action<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: &KeyEvent,
) -> ModalKeyResult {
    let selected = controller.state().active_modal().map_or(0, |m| m.selected);
    if selected == 3 {
        if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
            let current = current_thinking_from_modal(controller);
            let level = if current == "off" {
                None
            } else {
                Some(current.to_string())
            };
            return ModalKeyResult::ThinkingLevelSelected {
                level,
                save_as_default: true,
            };
        }
        if key.code == KeyCode::Left || key.code == KeyCode::Char('h') {
            return step_thinking_effort(controller, false);
        }
        if key.code == KeyCode::Right || key.code == KeyCode::Char('l') {
            return step_thinking_effort(controller, true);
        }
    }
    if key.code == KeyCode::Enter || key.code == KeyCode::Char(' ') {
        return toggle_selected_setting(controller, selected);
    }
    ModalKeyResult::Handled
}

pub fn handle_settings_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    if is_settings_exit(&key) {
        return pop_and_redraw_settings(controller);
    }
    let mut result = ModalKeyResult::Handled;
    if handle_arrow_nav(controller, &key) {
        // Navigated option
    } else if let KeyCode::Char(c) = key.code
        && c.is_ascii_digit()
        && c != '0'
        && !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        handle_digit_jump(controller, c);
    } else {
        result = handle_settings_action(controller, &key);
    }
    controller.redraw()?;
    Ok(result)
}
