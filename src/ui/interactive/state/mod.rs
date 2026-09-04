mod action;
pub mod autocomplete;
pub mod editor;
pub mod modal;
pub mod paste;
pub mod running_tool;
#[cfg(test)]
mod tests;
pub mod types;

pub use autocomplete::{AutocompleteItem, AutocompleteState};
pub use editor::EditorState;
pub use modal::{ModalMode, ModalOption, ModalState};
pub use running_tool::{MAX_RUNNING_BUFFER_BYTES, MAX_RUNNING_OUTPUT_BYTES, RunningTool};
pub use types::{Activity, FooterState, QueueKind, QueuedMessage, UiAction, UiEffect};

use std::collections::VecDeque;
use types::ModalFrame;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InteractiveState {
    editor: EditorState,
    footer: FooterState,
    active_tool: Option<RunningTool>,
    tools_expanded: bool,
    hide_thinking: bool,
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

    pub fn hide_thinking(&self) -> bool {
        self.hide_thinking
    }

    pub fn set_hide_thinking(&mut self, hide: bool) {
        self.hide_thinking = hide;
    }

    pub fn toggle_thinking(&mut self) -> bool {
        self.hide_thinking = !self.hide_thinking;
        self.hide_thinking
    }

    pub fn active_tool(&self) -> Option<&RunningTool> {
        self.active_tool.as_ref()
    }

    pub fn active_tool_mut(&mut self) -> Option<&mut RunningTool> {
        self.active_tool.as_mut()
    }

    pub fn set_active_tool(&mut self, tool: Option<RunningTool>) {
        self.active_tool = tool;
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
}
