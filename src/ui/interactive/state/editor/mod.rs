mod geometry;
mod history;
mod mutate;
mod navigation;

use history::EditorSnapshot;

use crate::ui::interactive::state::{
    paste::PasteStore,
    types::{QueueKind, QueuedMessage},
};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EditorState {
    text: String,
    cursor: usize,
    preferred_column: Option<usize>,
    kill_ring: Vec<String>,
    undo_stack: Vec<EditorSnapshot>,
    pastes: PasteStore,
}

impl EditorState {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn pastes(&self) -> &PasteStore {
        &self.pastes
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = self.text.len();
        self.pastes.sync_with_text(&self.text);
        self.preferred_column = None;
    }

    pub fn take_submission(&mut self, kind: QueueKind) -> Option<QueuedMessage> {
        let expanded = self.pastes.expand(&self.text);
        let text = expanded.trim().to_string();
        if text.is_empty() {
            return None;
        }
        self.text.clear();
        self.cursor = 0;
        self.pastes.clear();
        self.preferred_column = None;
        Some(QueuedMessage { text, kind })
    }
}
