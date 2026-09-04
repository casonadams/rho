use crate::ui::interactive::{AutocompleteState, EditorState, FooterState, ModalState, QueuedMessage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorPosition {
    pub row: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractiveLayout {
    pub lines: Vec<String>,
    pub cursor: CursorPosition,
    pub cursor_visible: bool,
    pub cursor_row: usize,
    pub queued_lines: Vec<String>,
    pub widget_lines: Vec<String>,
    pub working_line: String,
    pub top_divider: String,
    pub editor_lines: Vec<String>,
    pub bottom_divider: String,
    pub footer_lines: Vec<String>,
    pub footer: String,
}

impl InteractiveLayout {
    pub fn height(&self) -> usize {
        self.lines.len()
    }

    pub fn cursor_row(&self) -> usize {
        self.cursor_row
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LayoutInput<'a> {
    pub editor: &'a EditorState,
    pub modal: Option<&'a ModalState>,
    pub autocomplete: Option<&'a AutocompleteState>,
    pub footer: &'a FooterState,
    pub queued_messages: &'a [QueuedMessage],
    pub widget_lines: &'a [String],
    pub terminal_width: usize,
    pub spinner_frame: usize,
}
