use super::{
    InteractiveState,
    types::{UiAction, UiEffect},
};

fn apply_editor_action(editor: &mut super::EditorState, action: UiAction) {
    match action {
        UiAction::Insert(value) => editor.insert(value),
        UiAction::InsertNewline => editor.insert_newline(),
        UiAction::Backspace => editor.backspace(),
        UiAction::Delete => editor.delete(),
        UiAction::MoveLeft => editor.move_left(),
        UiAction::MoveRight => editor.move_right(),
        UiAction::MoveWordLeft => editor.move_word_left(),
        UiAction::MoveWordRight => editor.move_word_right(),
        UiAction::MoveToStart => editor.move_to_start(),
        UiAction::MoveToEnd => editor.move_to_end(),
        UiAction::DeleteWordBackward => editor.delete_word_backward(),
        UiAction::DeleteWordForward => editor.delete_word_forward(),
        UiAction::DeleteToLineStart => editor.delete_to_line_start(),
        UiAction::DeleteToLineEnd => editor.delete_to_line_end(),
        UiAction::Yank => editor.yank(),
        UiAction::Undo => editor.undo(),
        UiAction::Paste(text) => editor.handle_paste(&text),
        UiAction::Submit(_) | UiAction::Exit => {}
    }
}

impl InteractiveState {
    pub fn apply(&mut self, action: UiAction) -> UiEffect {
        match action {
            UiAction::Submit(kind) => self.handle_submit(kind),
            UiAction::Exit => UiEffect::Exit,
            other => {
                apply_editor_action(&mut self.editor, other);
                UiEffect::None
            }
        }
    }

    fn handle_submit(&mut self, kind: crate::ui::interactive::QueueKind) -> UiEffect {
        self.system_message = None;
        if let Some(message) = self.editor.take_submission(kind) {
            self.queue.push_back(message.clone());
            UiEffect::Queued(message)
        } else {
            UiEffect::None
        }
    }
}
