use crate::error::Result;
use crate::ui::interactive::{ModalKind, ModalOption, ModalState, TerminalBackend, TerminalController};
use crate::ui::theme::{BlockStyle, CursorMode};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_harness_core::thinking::ThinkingLevel;

use super::ModalKeyResult;

const TITLE: &str = "Settings";
const TITLE_WIDTH: usize = 21;

/// Rows of the settings modal in display order; a row's position is its option index.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
    BlockStyle,
    BoxResponses,
    ThinkingEffort,
    ThinkingOutput,
    ToolOutput,
    VersionBanner,
    CursorStyle,
    Models,
    ToolsAndPermissions,
}

impl Row {
    const ALL: [Row; 9] = [
        Row::BlockStyle,
        Row::BoxResponses,
        Row::ThinkingEffort,
        Row::ThinkingOutput,
        Row::ToolOutput,
        Row::VersionBanner,
        Row::CursorStyle,
        Row::Models,
        Row::ToolsAndPermissions,
    ];

    fn index(self) -> usize {
        Self::ALL.iter().position(|row| *row == self).unwrap_or_default()
    }

    fn from_index(index: usize) -> Option<Row> {
        Self::ALL.get(index).copied()
    }

    fn title(self) -> &'static str {
        match self {
            Row::BlockStyle => "Block Style",
            Row::BoxResponses => "Box Responses",
            Row::ThinkingEffort => "Thinking Effort",
            Row::ThinkingOutput => "Thinking Output",
            Row::ToolOutput => "Tool Output",
            Row::VersionBanner => "Version Banner",
            Row::CursorStyle => "Cursor Style",
            Row::Models => "Models",
            Row::ToolsAndPermissions => "Tools & Permissions",
        }
    }
}

fn block_style_label(style: BlockStyle) -> &'static str {
    match style {
        BlockStyle::Border => "Border",
        BlockStyle::Solid => "Solid",
        BlockStyle::Divider => "Divider",
    }
}

fn cursor_label(mode: CursorMode) -> &'static str {
    match mode {
        CursorMode::Software => "Software",
        CursorMode::Hardware => "Hardware",
    }
}

fn on_off(on: bool) -> &'static str {
    if on { "On" } else { "Off" }
}

fn shown_hidden(shown: bool) -> &'static str {
    if shown { "Shown" } else { "Hidden" }
}

fn expanded_collapsed(expanded: bool) -> &'static str {
    if expanded { "Expanded" } else { "Collapsed" }
}

fn model_description(model: &str) -> String {
    format!("{model}  ›")
}

fn option(row: Row, status: impl Into<String>) -> ModalOption {
    ModalOption::new(format!("{:<TITLE_WIDTH$}", row.title()), Some(status.into()))
}

pub fn open_settings_selector<B: TerminalBackend>(
    model: Option<&str>,
    _guard_model: Option<&str>,
    thinking_level: Option<&str>,
    controller: &mut TerminalController<B>,
) {
    let state = controller.state();
    let (hide_thinking, tools_expanded, show_label) =
        (state.hide_thinking(), state.tools_expanded(), state.show_label());
    let model_desc = model.map_or("›".to_string(), model_description);

    let options = Row::ALL
        .iter()
        .map(|row| match row {
            Row::BlockStyle => option(*row, block_style_label(controller.block_style())),
            Row::BoxResponses => option(*row, on_off(controller.block_agent_output())),
            Row::ThinkingEffort => option(*row, ThinkingLevel::from_setting(thinking_level).as_str()),
            Row::ThinkingOutput => option(*row, shown_hidden(!hide_thinking)),
            Row::ToolOutput => option(*row, expanded_collapsed(tools_expanded)),
            Row::VersionBanner => option(*row, shown_hidden(show_label)),
            Row::CursorStyle => option(*row, cursor_label(controller.cursor_mode())),
            Row::Models => option(*row, model_desc.clone()),
            Row::ToolsAndPermissions => option(*row, "Configure  ›"),
        })
        .collect();

    controller
        .state_mut()
        .push_modal(ModalState::new(TITLE, "", options).with_kind(ModalKind::Settings));
}

pub fn update_settings_model_description<B: TerminalBackend>(controller: &mut TerminalController<B>, model_name: &str) {
    update_setting_description(controller, &model_description(model_name), Row::Models);
}

fn update_setting_description<B: TerminalBackend>(controller: &mut TerminalController<B>, status: &str, row: Row) {
    controller
        .state_mut()
        .update_modal_option_desc(&ModalKind::Settings, row.index(), status);
}

fn current_thinking_from_modal<B: TerminalBackend>(controller: &TerminalController<B>) -> ThinkingLevel {
    let shown = controller
        .state()
        .active_modal()
        .and_then(|m| m.options.get(Row::ThinkingEffort.index()))
        .and_then(|o| o.description.as_deref());
    ThinkingLevel::from_setting(shown)
}

fn step_thinking_effort<B: TerminalBackend>(controller: &mut TerminalController<B>, forward: bool) -> ModalKeyResult {
    let current = current_thinking_from_modal(controller);
    let target = if forward { current.next() } else { current.prev() };
    update_setting_description(controller, target.as_str(), Row::ThinkingEffort);
    ModalKeyResult::ThinkingLevelSelected {
        level: target.setting(),
        save_as_default: true,
    }
}

fn toggle_ui_setting<B: TerminalBackend>(controller: &mut TerminalController<B>, row: Row) -> Option<ModalKeyResult> {
    match row {
        Row::BlockStyle => {
            let next = controller.toggle_block_style().unwrap_or(BlockStyle::Solid);
            let label = block_style_label(next);
            update_setting_description(controller, label, row);
            Some(ModalKeyResult::BlockStyleToggled {
                style: label.to_lowercase(),
            })
        }
        Row::BoxResponses => {
            let boxed = controller.toggle_block_agent_output().unwrap_or(false);
            update_setting_description(controller, on_off(boxed), row);
            Some(ModalKeyResult::AgentBoxToggled { boxed })
        }
        Row::VersionBanner => {
            let shown = controller.state_mut().toggle_show_label();
            update_setting_description(controller, shown_hidden(shown), row);
            Some(ModalKeyResult::ShowLabelToggled { shown })
        }
        Row::CursorStyle => {
            let next = controller.toggle_cursor_mode().unwrap_or(CursorMode::Hardware);
            let label = cursor_label(next);
            update_setting_description(controller, label, row);
            Some(ModalKeyResult::CursorToggled {
                cursor: label.to_lowercase(),
            })
        }
        _ => None,
    }
}

fn toggle_modal_setting(row: Row) -> Option<ModalKeyResult> {
    match row {
        Row::Models => Some(ModalKeyResult::OpenModelsMenu),
        Row::ToolsAndPermissions => Some(ModalKeyResult::OpenToolsMenu),
        _ => None,
    }
}

fn toggle_feature_setting<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    row: Row,
) -> Option<ModalKeyResult> {
    match row {
        Row::ThinkingEffort => Some(step_thinking_effort(controller, true)),
        Row::ThinkingOutput => {
            let hide = controller
                .toggle_thinking()
                .unwrap_or_else(|_| controller.state_mut().toggle_thinking());
            update_setting_description(controller, shown_hidden(!hide), row);
            Some(ModalKeyResult::ThinkingOutputToggled { hidden: hide })
        }
        Row::ToolOutput => {
            let expanded = controller
                .toggle_tools_expanded()
                .unwrap_or_else(|_| controller.state_mut().toggle_tools_expanded());
            update_setting_description(controller, expanded_collapsed(expanded), row);
            Some(ModalKeyResult::ToolOutputToggled { expanded })
        }
        _ => None,
    }
}

fn toggle_selected_setting<B: TerminalBackend>(controller: &mut TerminalController<B>, row: Row) -> ModalKeyResult {
    toggle_ui_setting(controller, row)
        .or_else(|| toggle_modal_setting(row))
        .or_else(|| toggle_feature_setting(controller, row))
        .unwrap_or(ModalKeyResult::Handled)
}

fn handle_thinking_effort_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: &KeyEvent,
) -> Option<ModalKeyResult> {
    if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(ModalKeyResult::ThinkingLevelSelected {
            level: current_thinking_from_modal(controller).setting(),
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
    let Some(row) = Row::from_index(selected) else {
        return ModalKeyResult::Handled;
    };
    if row == Row::ThinkingEffort
        && let Some(res) = handle_thinking_effort_key(controller, key)
    {
        return res;
    }
    if key.code == KeyCode::Enter || key.code == KeyCode::Char(' ') {
        return toggle_selected_setting(controller, row);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_keep_their_order_and_indices_match_positions() {
        let titles: Vec<&str> = Row::ALL.iter().map(|row| row.title()).collect();
        assert_eq!(
            titles,
            [
                "Block Style",
                "Box Responses",
                "Thinking Effort",
                "Thinking Output",
                "Tool Output",
                "Version Banner",
                "Cursor Style",
                "Models",
                "Tools & Permissions"
            ]
        );
        for (position, row) in Row::ALL.iter().enumerate() {
            assert_eq!(row.index(), position);
            assert!(Row::from_index(position) == Some(*row));
        }
        assert!(Row::from_index(Row::ALL.len()).is_none());
    }

    #[test]
    fn option_labels_are_padded_to_one_width_so_descriptions_align() {
        for row in Row::ALL {
            assert_eq!(option(row, "x").label.chars().count(), TITLE_WIDTH, "{}", row.title());
        }
    }

    #[test]
    fn status_labels() {
        assert_eq!(
            (on_off(true), on_off(false), shown_hidden(true), shown_hidden(false)),
            ("On", "Off", "Shown", "Hidden")
        );
        assert_eq!(
            (expanded_collapsed(true), expanded_collapsed(false)),
            ("Expanded", "Collapsed")
        );
        assert_eq!(
            (
                block_style_label(BlockStyle::Border),
                cursor_label(CursorMode::Hardware)
            ),
            ("Border", "Hardware")
        );
    }
}
