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
    guard_model: Option<&str>,
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
    let cursor_mode = match controller.cursor_mode() {
        crate::ui::theme::CursorMode::Software => "Software",
        crate::ui::theme::CursorMode::Hardware => "Hardware",
    };

    let model_name = model.unwrap_or("default");
    let guard_name = guard_model.unwrap_or("None");
    let thinking_effort = thinking_level.unwrap_or("off");
    let thinking_status = if hide_thinking { "Hidden" } else { "Shown" };
    let tools_status = if tools_expanded { "Expanded" } else { "Collapsed" };
    let agent_status = if boxed { "On" } else { "Off" };
    let label_status = if show_label { "Shown" } else { "Hidden" };

    let options = vec![
        ModalOption::new("Block Style       ", Some(block_style.to_string())),
        ModalOption::new("Box Responses     ", Some(agent_status.to_string())),
        ModalOption::new("Model             ", Some(model_name.to_string())),
        ModalOption::new("Guard Model       ", Some(guard_name.to_string())),
        ModalOption::new("Thinking Effort   ", Some(thinking_effort.to_string())),
        ModalOption::new("Thinking Output   ", Some(thinking_status.to_string())),
        ModalOption::new("Tool Output       ", Some(tools_status.to_string())),
        ModalOption::new("Version Banner    ", Some(label_status.to_string())),
        ModalOption::new("Cursor Style      ", Some(cursor_mode.to_string())),
        ModalOption::new("Tools & Permissions", None::<&str>),
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
        .and_then(|m| m.options.get(4))
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
    update_setting_description(controller, (target, 4));
    ModalKeyResult::ThinkingLevelSelected {
        level: if target == "off" {
            None
        } else {
            Some(target.to_string())
        },
        save_as_default: true,
    }
}

fn toggle_ui_setting<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    selected: usize,
) -> Option<ModalKeyResult> {
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
            Some(ModalKeyResult::BlockStyleToggled {
                style: label.to_lowercase(),
            })
        }
        1 => {
            let boxed = controller.toggle_block_agent_output().unwrap_or(false);
            update_setting_description(controller, (if boxed { "On" } else { "Off" }, 1));
            Some(ModalKeyResult::AgentBoxToggled { boxed })
        }
        7 => {
            let shown = controller.state_mut().toggle_show_label();
            update_setting_description(controller, (if shown { "Shown" } else { "Hidden" }, 7));
            Some(ModalKeyResult::ShowLabelToggled { shown })
        }
        8 => {
            let next = controller
                .toggle_cursor_mode()
                .unwrap_or(crate::ui::theme::CursorMode::Hardware);
            let label = match next {
                crate::ui::theme::CursorMode::Software => "Software",
                crate::ui::theme::CursorMode::Hardware => "Hardware",
            };
            update_setting_description(controller, (label, 8));
            Some(ModalKeyResult::CursorToggled {
                cursor: label.to_lowercase(),
            })
        }
        _ => None,
    }
}

fn toggle_modal_setting<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    selected: usize,
) -> Option<ModalKeyResult> {
    match selected {
        2 => {
            controller.state_mut().pop_modal();
            let _ = controller.redraw();
            Some(ModalKeyResult::OpenModelSelector { save_as_default: true })
        }
        3 => {
            controller.state_mut().pop_modal();
            let _ = controller.redraw();
            Some(ModalKeyResult::OpenGuardModelSelector)
        }
        9 => Some(ModalKeyResult::OpenToolsMenu),
        _ => None,
    }
}

fn toggle_feature_setting<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    selected: usize,
) -> Option<ModalKeyResult> {
    match selected {
        4 => Some(step_thinking_effort(controller, true)),
        5 => {
            let hide = controller
                .toggle_thinking()
                .unwrap_or_else(|_| controller.state_mut().toggle_thinking());
            update_setting_description(controller, (if hide { "Hidden" } else { "Shown" }, 5));
            Some(ModalKeyResult::ThinkingOutputToggled { hidden: hide })
        }
        6 => {
            let expanded = controller
                .toggle_tools_expanded()
                .unwrap_or_else(|_| controller.state_mut().toggle_tools_expanded());
            update_setting_description(controller, (if expanded { "Expanded" } else { "Collapsed" }, 6));
            Some(ModalKeyResult::ToolOutputToggled { expanded })
        }
        _ => None,
    }
}

fn toggle_selected_setting<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    selected: usize,
) -> ModalKeyResult {
    if let Some(res) = toggle_ui_setting(controller, selected) {
        return res;
    }
    if let Some(res) = toggle_modal_setting(controller, selected) {
        return res;
    }
    toggle_feature_setting(controller, selected).unwrap_or(ModalKeyResult::Handled)
}

fn handle_thinking_effort_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: &KeyEvent,
) -> Option<ModalKeyResult> {
    if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
        let current = current_thinking_from_modal(controller);
        let level = if current == "off" {
            None
        } else {
            Some(current.to_string())
        };
        return Some(ModalKeyResult::ThinkingLevelSelected {
            level,
            save_as_default: true,
        });
    }
    if key.code == KeyCode::Left || key.code == KeyCode::Char('h') {
        return Some(step_thinking_effort(controller, false));
    }
    if key.code == KeyCode::Right || key.code == KeyCode::Char('l') {
        return Some(step_thinking_effort(controller, true));
    }
    None
}

fn handle_settings_action<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: &KeyEvent,
) -> ModalKeyResult {
    let selected = controller.state().active_modal().map_or(0, |m| m.selected);
    if selected == 4
        && let Some(res) = handle_thinking_effort_key(controller, key)
    {
        return res;
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
    if key.code == KeyCode::Esc || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)) {
        super::pop_and_cancel(controller)?;
        return Ok(ModalKeyResult::Handled);
    }
    if super::handle_selector_nav(controller, &key)? {
        Ok(ModalKeyResult::Handled)
    } else {
        let result = handle_settings_action(controller, &key);
        controller.redraw()?;
        Ok(result)
    }
}
