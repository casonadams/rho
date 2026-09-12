pub mod help;
pub mod interaction;
pub mod login;
pub mod mcp;
pub mod model;
pub mod session;
pub mod settings;
#[cfg(test)]
mod tests;
pub mod tree;

use crate::error::Result;
use crate::ui::interactive::{EditorState, ModalMode, TerminalBackend, TerminalController, UiAction};
use crossterm::event::KeyEvent;

pub use help::open_help_selector;
pub use interaction::{PendingModal, install_interaction};
pub use login::open_login_selector;
pub use mcp::open_mcp_selector;
pub use model::open_model_selector;
pub use session::open_session_selector;
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
    OpenModelSelector,
    BlockStyleToggled {
        style: String,
    },
    AgentBoxToggled {
        boxed: bool,
    },
    ShowLabelToggled {
        shown: bool,
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
        "Help" => help::handle_help_key(controller, key),
        "Settings" => settings::handle_settings_key(controller, key),
        "Resume Session" => session::handle_session_key(controller, key),
        "Conversation Tree" => tree::handle_tree_key(controller, key),
        "Select Model" => model::handle_model_key(controller, key),
        "Model Context Protocol" => mcp::handle_mcp_key(controller, key),
        "Login Provider" => login::handle_login_key(controller, key),
        _ => interaction::handle_interaction_key(controller, key, pending),
    }
}
