use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueKind {
    Steering,
    FollowUp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedMessage {
    pub text: String,
    pub kind: QueueKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Activity {
    #[default]
    Idle,
    Thinking,
    Tool(String),
}

impl Activity {
    pub fn label(&self) -> &str {
        match self {
            Self::Idle => "idle",
            Self::Thinking => "thinking",
            Self::Tool(label) => label,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FooterState {
    pub activity: Activity,
    pub model: String,
    pub context: Option<String>,
    pub quota: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModalState {
    pub title: String,
    pub body: String,
    pub options: Vec<String>,
    pub selected: usize,
}

impl ModalState {
    pub fn new(title: impl Into<String>, body: impl Into<String>, options: Vec<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            options,
            selected: 0,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EditorState {
    text: String,
    cursor: usize,
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

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = self.text.len();
    }

    pub fn insert(&mut self, value: char) {
        self.text.insert(self.cursor, value);
        self.cursor += value.len_utf8();
    }

    pub fn insert_newline(&mut self) {
        self.insert('\n');
    }

    pub fn backspace(&mut self) {
        let Some((index, _)) = self.text[..self.cursor].char_indices().next_back() else {
            return;
        };
        self.text.drain(index..self.cursor);
        self.cursor = index;
    }

    pub fn delete(&mut self) {
        let Some(character) = self.text[self.cursor..].chars().next() else {
            return;
        };
        self.text.drain(self.cursor..self.cursor + character.len_utf8());
    }

    pub fn move_left(&mut self) {
        if let Some((index, _)) = self.text[..self.cursor].char_indices().next_back() {
            self.cursor = index;
        }
    }

    pub fn move_right(&mut self) {
        if let Some(character) = self.text[self.cursor..].chars().next() {
            self.cursor += character.len_utf8();
        }
    }

    pub fn move_to_start(&mut self) {
        self.cursor = 0;
    }

    pub fn move_to_end(&mut self) {
        self.cursor = self.text.len();
    }

    pub fn take_submission(&mut self, kind: QueueKind) -> Option<QueuedMessage> {
        let text = self.text.trim().to_string();
        if text.is_empty() {
            return None;
        }
        self.text.clear();
        self.cursor = 0;
        Some(QueuedMessage { text, kind })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiAction {
    Insert(char),
    InsertNewline,
    Backspace,
    Delete,
    MoveLeft,
    MoveRight,
    MoveToStart,
    MoveToEnd,
    Submit(QueueKind),
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiEffect {
    None,
    Queued(QueuedMessage),
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModalFrame {
    modal: ModalState,
    saved_editor: EditorState,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InteractiveState {
    editor: EditorState,
    footer: FooterState,
    queue: VecDeque<QueuedMessage>,
    modals: Vec<ModalFrame>,
}

impl InteractiveState {
    pub fn editor(&self) -> &EditorState {
        &self.editor
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

    pub fn pop_queued(&mut self) -> Option<QueuedMessage> {
        self.queue.pop_front()
    }

    pub fn active_modal(&self) -> Option<&ModalState> {
        self.modals.last().map(|frame| &frame.modal)
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
            UiAction::MoveToStart => self.editor.move_to_start(),
            UiAction::MoveToEnd => self.editor.move_to_end(),
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

#[cfg(test)]
mod tests {
    use super::{InteractiveState, ModalState, QueueKind, UiAction, UiEffect};

    #[test]
    fn editor_inserts_and_deletes_at_unicode_boundaries() {
        let mut state = InteractiveState::default();
        state.editor_mut().set_text("a界c");
        state.apply(UiAction::MoveLeft);
        state.apply(UiAction::Backspace);
        assert_eq!(state.editor().text(), "ac");
        assert_eq!(state.editor().cursor(), 1);

        state.apply(UiAction::Delete);
        assert_eq!(state.editor().text(), "a");
        assert_eq!(state.editor().cursor(), 1);
    }

    #[test]
    fn submissions_keep_fifo_order_and_classification() {
        let mut state = InteractiveState::default();
        state.editor_mut().set_text(" steer ");
        assert_eq!(
            state.apply(UiAction::Submit(QueueKind::Steering)),
            UiEffect::Queued(super::QueuedMessage {
                text: "steer".to_string(),
                kind: QueueKind::Steering,
            })
        );
        state.editor_mut().set_text("follow");
        state.apply(UiAction::Submit(QueueKind::FollowUp));

        assert_eq!(state.queue_len(), 2);
        assert_eq!(state.pop_queued().unwrap().kind, QueueKind::Steering);
        assert_eq!(state.pop_queued().unwrap().kind, QueueKind::FollowUp);
    }

    #[test]
    fn empty_submissions_are_ignored() {
        let mut state = InteractiveState::default();
        state.editor_mut().set_text(" \n ");
        assert_eq!(state.apply(UiAction::Submit(QueueKind::Steering)), UiEffect::None);
        assert_eq!(state.queue_len(), 0);
    }

    #[test]
    fn nested_modals_restore_each_saved_draft_without_changing_queue() {
        let mut state = InteractiveState::default();
        state.editor_mut().set_text("original draft");
        state.apply(UiAction::Submit(QueueKind::Steering));
        state.editor_mut().set_text("next draft");
        state.push_modal(ModalState::new("Approval", "Allow tool?", vec![]));
        state.editor_mut().set_text("modal response");
        state.push_modal(ModalState::new("Question", "Choose", vec!["One".into()]));
        state.editor_mut().set_text("custom answer");

        assert_eq!(state.active_modal().unwrap().title, "Question");
        state.pop_modal();
        assert_eq!(state.editor().text(), "modal response");
        state.pop_modal();
        assert_eq!(state.editor().text(), "next draft");
        assert_eq!(state.queue_len(), 1);
    }
}
