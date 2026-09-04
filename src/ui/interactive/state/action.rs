use super::{
    InteractiveState,
    types::{UiAction, UiEffect},
};

impl InteractiveState {
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
            UiAction::Paste(text) => self.editor.handle_paste(&text),
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
