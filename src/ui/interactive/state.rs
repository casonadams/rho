use std::{collections::VecDeque, time::Instant};

use unicode_width::UnicodeWidthChar;

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
    Working,
}

impl Activity {
    pub fn label(&self) -> &str {
        match self {
            Self::Idle => "idle",
            Self::Thinking => "thinking",
            Self::Working => "working",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningTool {
    pub command: String,
    pub started: Instant,
}

impl RunningTool {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            started: Instant::now(),
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
pub struct ModalOption {
    pub label: String,
    pub description: Option<String>,
}

impl ModalOption {
    pub fn new(label: impl Into<String>, description: Option<impl Into<String>>) -> Self {
        Self {
            label: label.into(),
            description: description.map(Into::into),
        }
    }
}

impl From<String> for ModalOption {
    fn from(label: String) -> Self {
        Self {
            label,
            description: None,
        }
    }
}

impl From<&str> for ModalOption {
    fn from(label: &str) -> Self {
        Self {
            label: label.to_string(),
            description: None,
        }
    }
}

impl From<crate::ui::interactive::InteractionOption> for ModalOption {
    fn from(opt: crate::ui::interactive::InteractionOption) -> Self {
        Self {
            label: opt.label,
            description: opt.description,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ModalMode {
    #[default]
    Select,
    Input {
        prompt_label: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModalState {
    pub title: String,
    pub body: String,
    pub options: Vec<ModalOption>,
    pub selected: usize,
    pub mode: ModalMode,
    pub input: EditorState,
    pub allow_custom: bool,
}

impl ModalState {
    pub fn new(title: impl Into<String>, body: impl Into<String>, options: Vec<ModalOption>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            options,
            selected: 0,
            mode: ModalMode::Select,
            input: EditorState::default(),
            allow_custom: false,
        }
    }

    pub fn with_custom(mut self, allow_custom: bool) -> Self {
        self.allow_custom = allow_custom;
        self
    }

    pub fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn select_next(&mut self) {
        if !self.options.is_empty() {
            self.selected = (self.selected + 1).min(self.options.len() - 1);
        }
    }

    pub fn selected_option(&self) -> Option<&ModalOption> {
        self.options.get(self.selected)
    }

    pub fn enter_input_mode(&mut self, prompt_label: impl Into<String>) {
        self.mode = ModalMode::Input {
            prompt_label: prompt_label.into(),
        };
        self.input.set_text("");
    }

    pub fn exit_input_mode(&mut self) {
        self.mode = ModalMode::Select;
        self.input.set_text("");
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EditorState {
    text: String,
    cursor: usize,
    preferred_column: Option<usize>,
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
        self.preferred_column = None;
    }

    pub fn insert(&mut self, value: char) {
        self.text.insert(self.cursor, value);
        self.cursor += value.len_utf8();
        self.preferred_column = None;
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
        self.preferred_column = None;
    }

    pub fn delete(&mut self) {
        let Some(character) = self.text[self.cursor..].chars().next() else {
            return;
        };
        self.text.drain(self.cursor..self.cursor + character.len_utf8());
        self.preferred_column = None;
    }

    pub fn move_left(&mut self) {
        if let Some((index, _)) = self.text[..self.cursor].char_indices().next_back() {
            self.cursor = index;
        }
        self.preferred_column = None;
    }

    pub fn move_right(&mut self) {
        if let Some(character) = self.text[self.cursor..].chars().next() {
            self.cursor += character.len_utf8();
        }
        self.preferred_column = None;
    }

    pub fn move_up(&mut self, terminal_width: usize) -> bool {
        self.move_vertical(terminal_width, -1)
    }

    pub fn move_down(&mut self, terminal_width: usize) -> bool {
        self.move_vertical(terminal_width, 1)
    }

    pub fn move_to_start(&mut self) {
        self.cursor = 0;
        self.preferred_column = None;
    }

    pub fn move_to_end(&mut self) {
        self.cursor = self.text.len();
        self.preferred_column = None;
    }

    fn move_vertical(&mut self, terminal_width: usize, row_delta: isize) -> bool {
        let terminal_width = terminal_width.max(1);
        let (current_row, current_column) = editor_cursor_position(&self.text, self.cursor, terminal_width);
        let Some(target_row) = current_row.checked_add_signed(row_delta) else {
            return false;
        };
        let preferred_column = self.preferred_column.unwrap_or(current_column);
        let target = editor_boundaries(&self.text)
            .map(|cursor| {
                let (row, column) = editor_cursor_position(&self.text, cursor, terminal_width);
                (cursor, row, column)
            })
            .filter(|(_, row, _)| *row == target_row)
            .min_by_key(|(_, _, column)| column.abs_diff(preferred_column));
        if let Some((cursor, _, _)) = target {
            self.cursor = cursor;
            self.preferred_column = Some(preferred_column);
            true
        } else {
            false
        }
    }

    pub fn take_submission(&mut self, kind: QueueKind) -> Option<QueuedMessage> {
        let text = self.text.trim().to_string();
        if text.is_empty() {
            return None;
        }
        self.text.clear();
        self.cursor = 0;
        self.preferred_column = None;
        Some(QueuedMessage { text, kind })
    }
}

fn editor_boundaries(text: &str) -> impl Iterator<Item = usize> + '_ {
    std::iter::once(0).chain(
        text.char_indices()
            .map(|(index, character)| index + character.len_utf8()),
    )
}

fn editor_cursor_position(text: &str, cursor: usize, terminal_width: usize) -> (usize, usize) {
    let mut row = 0;
    let mut column = 0;
    for (byte_index, character) in text.char_indices() {
        if character == '\n' {
            if byte_index == cursor {
                return (row, column);
            }
            row += 1;
            column = 0;
            continue;
        }
        let character_width = character.width().unwrap_or(0);
        if column > 0 && column + character_width > terminal_width {
            row += 1;
            column = 0;
        }
        if byte_index == cursor {
            return (row, column);
        }
        column += character_width;
    }
    if column == terminal_width {
        row += 1;
        column = 0;
    }
    (row, column)
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
    tools_expanded: bool,
    queue: VecDeque<QueuedMessage>,
    modals: Vec<ModalFrame>,
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

    pub fn pop_queued(&mut self) -> Option<QueuedMessage> {
        self.queue.pop_front()
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
    use super::{InteractiveState, ModalOption, ModalState, QueueKind, UiAction, UiEffect};

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
    fn vertical_movement_tracks_the_preferred_column_across_lines() {
        let mut state = InteractiveState::default();
        state.editor_mut().set_text("abcdef\nx\nabcdef");

        assert!(state.editor_mut().move_up(20));
        assert_eq!(state.editor().cursor(), 8);
        assert!(state.editor_mut().move_up(20));
        assert_eq!(state.editor().cursor(), 6);
        assert!(!state.editor_mut().move_up(20));
        assert!(state.editor_mut().move_down(20));
        assert_eq!(state.editor().cursor(), 8);
    }

    #[test]
    fn vertical_movement_uses_visual_wrapped_lines() {
        let mut state = InteractiveState::default();
        state.editor_mut().set_text("abcdefghi");

        assert!(state.editor_mut().move_up(4));
        assert_eq!(state.editor().cursor(), 5);
        assert!(state.editor_mut().move_up(4));
        assert_eq!(state.editor().cursor(), 1);
        assert!(!state.editor_mut().move_up(4));
        assert!(state.editor_mut().move_down(4));
        assert_eq!(state.editor().cursor(), 5);
    }

    #[test]
    fn vertical_movement_preserves_display_column_across_wide_and_short_lines() {
        let mut state = InteractiveState::default();
        state.editor_mut().set_text("a界bc\nx\na界bc");

        assert!(state.editor_mut().move_up(20));
        assert_eq!(state.editor().cursor(), 8);
        assert!(state.editor_mut().move_up(20));
        assert_eq!(state.editor().cursor(), 6);
        assert!(state.editor_mut().move_down(20));
        assert_eq!(state.editor().cursor(), 8);
        assert!(state.editor_mut().move_down(20));
        assert_eq!(state.editor().cursor(), state.editor().text().len());
        assert!(!state.editor_mut().move_down(20));
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
    fn tools_expanded_toggle_and_set() {
        let mut state = InteractiveState::default();
        assert!(!state.tools_expanded());

        assert!(state.toggle_tools_expanded());
        assert!(state.tools_expanded());

        assert!(!state.toggle_tools_expanded());
        assert!(!state.tools_expanded());

        state.set_tools_expanded(true);
        assert!(state.tools_expanded());
    }

    #[test]
    fn nested_modals_restore_each_saved_draft_without_changing_queue() {
        let mut state = InteractiveState::default();
        state.editor_mut().set_text("original draft");
        state.apply(UiAction::Submit(QueueKind::Steering));
        state.editor_mut().set_text("next draft");
        state.push_modal(ModalState::new("Approval", "Allow tool?", Vec::<ModalOption>::new()));
        state.editor_mut().set_text("modal response");
        state.push_modal(ModalState::new("Question", "Choose", vec![ModalOption::from("One")]));
        state.editor_mut().set_text("custom answer");

        assert_eq!(state.active_modal().unwrap().title, "Question");
        state.pop_modal();
        assert_eq!(state.editor().text(), "modal response");
        state.pop_modal();
        assert_eq!(state.editor().text(), "next draft");
        assert_eq!(state.queue_len(), 1);
    }
}
