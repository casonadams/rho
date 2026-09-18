pub mod interaction;
pub mod selectors;
pub mod settings;
#[cfg(test)]
mod tests;
pub mod tree;

use crate::error::Result;
use crate::ui::interactive::{EditorState, ModalMode, TerminalBackend, TerminalController, UiAction};
use crossterm::event::KeyEvent;

pub use interaction::{PendingModal, install_interaction};
pub use selectors::{
    open_help_selector, open_login_selector, open_mcp_selector, open_model_selector, open_model_selector_with_default,
    open_remote_modal, open_session_selector,
};
pub use settings::open_settings_selector;
pub use tree::open_tree_selector;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalKeyResult {
    NotHandled,
    Handled,
    ModelSelected {
        model: String,
        provider: String,
        save_as_default: bool,
    },
    TreeNodeSelected {
        node_id: String,
    },
    NodeLabelUpdated {
        node_id: String,
        label: String,
    },
    SessionSelected {
        session_id: String,
    },
    SessionDeleted {
        session_id: String,
    },
    ThinkingLevelSelected {
        level: Option<String>,
        save_as_default: bool,
    },
    LoginProviderSelected {
        provider: String,
    },
    McpServerToggled {
        server: String,
    },
    HelpCommandSelected {
        command: String,
    },
    OpenModelSelector {
        save_as_default: bool,
    },
    BlockStyleToggled {
        style: String,
    },
    AgentBoxToggled {
        boxed: bool,
    },
    ShowLabelToggled {
        shown: bool,
    },
    ThinkingOutputToggled {
        hidden: bool,
    },
    ToolOutputToggled {
        expanded: bool,
    },
    CursorToggled {
        cursor: String,
    },
    SemanticSearchToggled {
        enabled: bool,
    },
}

pub(crate) fn apply_input_edit(input: &mut EditorState, action: UiAction) {
    match action {
        UiAction::Insert(c) => input.insert(c),
        UiAction::InsertNewline => input.insert_newline(),
        UiAction::Backspace => input.backspace(),
        UiAction::Delete => input.delete(),
        UiAction::MoveLeft => input.move_left(),
        UiAction::MoveRight => input.move_right(),
        UiAction::MoveWordLeft => input.move_word_left(),
        UiAction::MoveWordRight => input.move_word_right(),
        UiAction::MoveToStart => input.move_to_start(),
        UiAction::MoveToEnd => input.move_to_end(),
        UiAction::DeleteWordBackward => input.delete_word_backward(),
        UiAction::DeleteWordForward => input.delete_word_forward(),
        UiAction::DeleteToLineStart => input.delete_to_line_start(),
        UiAction::DeleteToLineEnd => input.delete_to_line_end(),
        UiAction::Yank => input.yank(),
        UiAction::Undo => input.undo(),
        UiAction::Paste(text) => input.handle_paste(&text),
        _ => {}
    }
}

pub(crate) fn pop_and_cancel<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<()> {
    controller.state_mut().pop_modal();
    controller.redraw()?;
    Ok(())
}

pub(crate) fn apply_filter<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    character: Option<char>,
) -> Result<()> {
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
    Ok(())
}

pub(crate) fn clear_filter_or_cancel<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<bool> {
    let has_filter = controller
        .state()
        .active_modal()
        .is_some_and(|m| !m.filter_query.is_empty());
    if has_filter {
        if let Some(modal) = controller.state_mut().active_modal_mut() {
            modal.set_filter("");
        }
        controller.redraw()?;
        Ok(false)
    } else {
        pop_and_cancel(controller)?;
        Ok(true)
    }
}

pub(crate) fn handle_selector_nav<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: &KeyEvent,
) -> Result<bool> {
    use crossterm::event::{KeyCode, KeyModifiers};

    let is_searchable = controller.state().active_modal().is_some_and(|m| m.is_searchable);

    match key.code {
        KeyCode::Up | KeyCode::BackTab => {
            controller.state_mut().select_previous_modal_option();
            controller.redraw()?;
            Ok(true)
        }
        KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
            controller.state_mut().select_previous_modal_option();
            controller.redraw()?;
            Ok(true)
        }
        KeyCode::Down | KeyCode::Tab => {
            controller.state_mut().select_next_modal_option();
            controller.redraw()?;
            Ok(true)
        }
        KeyCode::Char('k') if !is_searchable && key.modifiers.is_empty() => {
            controller.state_mut().select_previous_modal_option();
            controller.redraw()?;
            Ok(true)
        }
        KeyCode::Char('j') if !is_searchable && key.modifiers.is_empty() => {
            controller.state_mut().select_next_modal_option();
            controller.redraw()?;
            Ok(true)
        }
        KeyCode::Char(c)
            if !is_searchable
                && c.is_ascii_digit()
                && c != '0'
                && !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            let idx = (c as usize).saturating_sub('1' as usize);
            let count = controller.state().active_modal().map_or(0, |m| m.options.len());
            if idx < count
                && let Some(modal) = controller.state_mut().active_modal_mut()
            {
                modal.selected = idx;
                controller.redraw()?;
            }
            Ok(true)
        }
        KeyCode::Backspace if is_searchable => {
            apply_filter(controller, None)?;
            Ok(true)
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            clear_filter_or_cancel(controller)?;
            Ok(true)
        }
        KeyCode::Char(c) if is_searchable && !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
            apply_filter(controller, Some(c))?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

pub(crate) fn dispatch_simple_selector<B: TerminalBackend, F>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    on_enter: F,
) -> Result<ModalKeyResult>
where
    F: FnOnce(&crate::ui::interactive::ModalOption) -> Option<ModalKeyResult>,
{
    use crossterm::event::KeyCode;

    match key.code {
        KeyCode::Enter => {
            let selected = controller
                .state()
                .active_modal()
                .and_then(|m| m.selected_option())
                .cloned();
            pop_and_cancel(controller)?;
            Ok(selected.as_ref().and_then(on_enter).unwrap_or(ModalKeyResult::Handled))
        }
        KeyCode::Esc => {
            pop_and_cancel(controller)?;
            Ok(ModalKeyResult::Handled)
        }
        _ => {
            handle_selector_nav(controller, &key)?;
            Ok(ModalKeyResult::Handled)
        }
    }
}

pub fn handle_modal_paste<B: TerminalBackend>(controller: &mut TerminalController<B>, text: &str) -> bool {
    let Some(active) = controller.state().active_modal() else {
        return false;
    };
    let is_input_mode = matches!(active.mode, ModalMode::Input { .. });
    let is_searchable = active.is_searchable;
    let allow_custom = active.allow_custom;
    let title = active.title.clone();
    let filter_query = active.filter_query.clone();
    let selected = active.selected;
    let option_input = active.options.get(selected).and_then(|o| o.input.clone());

    if is_input_mode {
        if let Some(modal) = controller.state_mut().active_modal_mut() {
            apply_input_edit(&mut modal.input, UiAction::Paste(text.to_string()));
        }
    } else if is_searchable {
        let mut query = filter_query;
        query.push_str(text);
        if let Some(modal) = controller.state_mut().active_modal_mut() {
            modal.set_filter(&query);
        }
    } else if allow_custom {
        let prompt = interaction::prompt_label_for(&title);
        if let Some(modal) = controller.state_mut().active_modal_mut() {
            modal.enter_input_mode(prompt);
            apply_input_edit(&mut modal.input, UiAction::Paste(text.to_string()));
        }
    } else if let Some(spec) = option_input
        && let Some(modal) = controller.state_mut().active_modal_mut()
    {
        modal.selected = selected;
        modal.input_option = Some(selected);
        modal.enter_input_mode(&spec.label);
        if let Some(prefill) = spec.value {
            modal.input.set_text(prefill);
        }
        apply_input_edit(&mut modal.input, UiAction::Paste(text.to_string()));
    }
    true
}

pub fn handle_modal_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    pending: &mut Option<PendingModal>,
) -> Result<ModalKeyResult> {
    let Some(active) = controller.state().active_modal() else {
        return Ok(ModalKeyResult::NotHandled);
    };

    if key.kind == crossterm::event::KeyEventKind::Release {
        return Ok(ModalKeyResult::Handled);
    }

    match active.title.as_str() {
        "Help" => selectors::handle_help_key(controller, key),
        "Settings" => settings::handle_settings_key(controller, key),
        "Resume Session" => selectors::handle_session_key(controller, key),
        "Conversation Tree" => tree::handle_tree_key(controller, key),
        "Select Model" => selectors::handle_model_key(controller, key),
        "Model Context Protocol" => selectors::handle_mcp_key(controller, key),
        "Login Provider" => selectors::handle_login_key(controller, key),
        "Remote Access" => selectors::handle_remote_key(controller, key),
        _ => interaction::handle_interaction_key(controller, key, pending),
    }
}
