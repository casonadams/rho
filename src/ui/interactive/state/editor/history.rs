use super::EditorState;
use crate::ui::interactive::state::paste::PasteStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EditorSnapshot {
    pub(super) text: String,
    pub(super) cursor: usize,
    pub(super) pastes: PasteStore,
}

impl EditorState {
    pub fn yank(&mut self) {
        if let Some(last) = self.kill_ring.last().cloned() {
            self.record_undo();
            self.text.insert_str(self.cursor, &last);
            self.cursor += last.len();
            self.preferred_column = None;
        }
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.text = prev.text;
            self.cursor = prev.cursor.min(self.text.len());
            self.pastes = prev.pastes;
            self.preferred_column = None;
        }
    }

    pub(super) fn record_undo(&mut self) {
        if self
            .undo_stack
            .last()
            .map(|s| s.text != self.text || s.cursor != self.cursor || s.pastes != self.pastes)
            .unwrap_or(true)
        {
            if self.undo_stack.len() >= 50 {
                self.undo_stack.remove(0);
            }
            self.undo_stack.push(EditorSnapshot {
                text: self.text.clone(),
                cursor: self.cursor,
                pastes: self.pastes.clone(),
            });
        }
    }
}
