pub mod interaction;
pub mod model;
pub mod session;
pub mod settings;
#[cfg(test)]
mod tests;
pub mod tree;

use crate::error::Result;
use crate::ui::interactive::{EditorState, TerminalBackend, TerminalController, UiAction};
use crossterm::event::KeyEvent;

pub use interaction::{PendingModal, install_interaction};
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
}

pub(crate) fn apply_input_edit(input: &mut EditorState, action: UiAction) {
    match action {
        UiAction::Insert(c) => input.insert(c),
        UiAction::Backspace => input.backspace(),
        UiAction::Delete => input.delete(),
        UiAction::MoveLeft => input.move_left(),
        UiAction::MoveRight => input.move_right(),
        UiAction::MoveToStart => input.move_to_start(),
        UiAction::MoveToEnd => input.move_to_end(),
        _ => {}
    }
}

pub fn handle_modal_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    pending: &mut Option<PendingModal>,
) -> Result<ModalKeyResult> {
    let Some(active) = controller.state().active_modal() else {
        return Ok(ModalKeyResult::NotHandled);
    };

    match active.title.as_str() {
        "Settings" => settings::handle_settings_key(controller, key),
        "Resume Session" => session::handle_session_key(controller, key),
        "Conversation Tree" => tree::handle_tree_key(controller, key),
        "Select Model" => model::handle_model_key(controller, key),
        _ => interaction::handle_interaction_key(controller, key, pending),
    }
}
