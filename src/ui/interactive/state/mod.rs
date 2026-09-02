pub mod autocomplete;
pub mod editor;
pub mod modal;
#[cfg(test)]
mod tests;
pub mod types;

pub use autocomplete::{AutocompleteItem, AutocompleteState};
pub use editor::EditorState;
pub use modal::{ModalMode, ModalOption, ModalState};
pub use types::{Activity, FooterState, QueueKind, QueuedMessage, RunningTool, UiAction, UiEffect};

use std::collections::VecDeque;
use types::ModalFrame;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InteractiveState {
    editor: EditorState,
    footer: FooterState,
    tools_expanded: bool,
    queue: VecDeque<QueuedMessage>,
    modals: Vec<ModalFrame>,
    pub autocomplete: AutocompleteState,
}

impl InteractiveState {
    pub fn editor(&self) -> &EditorState {
        &self.editor
    }

    pub fn tools_expanded(&self) -> bool {
        self.tools_expanded
    }

    pub fn set_tools_expanded(&mut self, expanded: bool) {
        self.tools_expanded = expanded;
    }

    pub fn toggle_tools_expanded(&mut self) -> bool {
        self.tools_expanded = !self.tools_expanded;
        self.tools_expanded
    }

    pub fn editor_mut(&mut self) -> &mut EditorState {
        &mut self.editor
    }

    pub fn footer(&self) -> &FooterState {
        &self.footer
    }

    pub fn footer_mut(&mut self) -> &mut FooterState {
        &mut self.footer
    }

    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    pub fn queue(&self) -> &VecDeque<QueuedMessage> {
        &self.queue
    }

    pub fn dequeue_all(&mut self) -> Vec<QueuedMessage> {
        self.queue.drain(..).collect()
    }

    pub fn pop_queued(&mut self) -> Option<QueuedMessage> {
        self.queue.pop_front()
    }

    pub fn push_front_queued(&mut self, message: QueuedMessage) {
        self.queue.push_front(message);
    }

    pub fn clear_queue(&mut self) {
        self.queue.clear();
    }

    pub fn active_modal(&self) -> Option<&ModalState> {
        self.modals.last().map(|frame| &frame.modal)
    }

    pub fn active_modal_mut(&mut self) -> Option<&mut ModalState> {
        self.modals.last_mut().map(|frame| &mut frame.modal)
    }

    pub fn select_previous_modal_option(&mut self) {
        if let Some(modal) = self.modals.last_mut().map(|frame| &mut frame.modal) {
            modal.select_previous();
        }
    }

    pub fn select_next_modal_option(&mut self) {
        if let Some(modal) = self.modals.last_mut().map(|frame| &mut frame.modal) {
            modal.select_next();
        }
    }

    pub fn push_modal(&mut self, modal: ModalState) {
        let saved_editor = std::mem::take(&mut self.editor);
        self.modals.push(ModalFrame { modal, saved_editor });
    }

    pub fn pop_modal(&mut self) -> Option<ModalState> {
        let frame = self.modals.pop()?;
        self.editor = frame.saved_editor;
        Some(frame.modal)
    }

    pub fn apply(&mut self, action: UiAction) -> UiEffect {
        match action {
            UiAction::Insert(value) => self.editor.insert(value),
            UiAction::InsertNewline => self.editor.insert_newline(),
            UiAction::Backspace => self.editor.backspace(),
            UiAction::Delete => self.editor.delete(),
            UiAction::MoveLeft => self.editor.move_left(),
            UiAction::MoveRight => self.editor.move_right(),
            UiAction::MoveWordLeft => self.editor.move_word_left(),
            UiAction::MoveWordRight => self.editor.move_word_right(),
            UiAction::MoveToStart => self.editor.move_to_start(),
            UiAction::MoveToEnd => self.editor.move_to_end(),
            UiAction::DeleteWordBackward => self.editor.delete_word_backward(),
            UiAction::DeleteWordForward => self.editor.delete_word_forward(),
            UiAction::DeleteToLineStart => self.editor.delete_to_line_start(),
            UiAction::DeleteToLineEnd => self.editor.delete_to_line_end(),
            UiAction::Yank => self.editor.yank(),
            UiAction::Undo => self.editor.undo(),
            UiAction::Submit(kind) => {
                if let Some(message) = self.editor.take_submission(kind) {
                    self.queue.push_back(message.clone());
                    return UiEffect::Queued(message);
                }
            }
            UiAction::Exit => return UiEffect::Exit,
        }
        UiEffect::None
    }
}
