use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui_textarea::{CursorMove, Input, Key, TextArea};

use super::{PromptEditor, TerminalComponent};
use crate::platform::clipboard;
use crate::ui::interactive::{PasteStore, QueueKind, QueuedMessage, check_paste_threshold, sanitize_paste};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorMode {
    #[default]
    Default,
    Vim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VimMode {
    #[default]
    Normal,
    Insert,
    Visual,
    VisualLine,
    Replace(bool),
    Operator(char),
}

impl VimMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Visual => "VISUAL",
            Self::VisualLine => "VISUAL LINE",
            Self::Replace(true) => "REPLACE ONCE",
            Self::Replace(false) => "REPLACE",
            Self::Operator('d') => "DELETE",
            Self::Operator('y') => "YANK",
            Self::Operator('c') => "CHANGE",
            Self::Operator(_) => "OPERATOR",
        }
    }

    pub fn cursor_style(&self) -> Style {
        let color = match self {
            Self::Normal => Color::Reset,
            Self::Insert => Color::LightBlue,
            Self::Visual | Self::VisualLine => Color::LightYellow,
            Self::Replace(_) => Color::LightRed,
            Self::Operator(_) => Color::LightGreen,
        };
        Style::default().fg(color).add_modifier(Modifier::REVERSED)
    }
}

pub enum VimTransition {
    Nop,
    Mode(VimMode),
    Pending(Input),
}

#[derive(Debug, Clone, Default)]
pub struct Vim {
    pub mode: VimMode,
    pub pending: Option<Input>,
}

impl Vim {
    pub fn new(mode: VimMode) -> Self {
        Self { mode, pending: None }
    }

    fn is_before_line_end(textarea: &TextArea<'_>) -> bool {
        let cursor = textarea.cursor();
        let lines = textarea.lines();
        if cursor.0 < lines.len() {
            cursor.1 < lines[cursor.0].len().saturating_sub(1)
        } else {
            false
        }
    }

    pub fn transition(&mut self, input: Input, textarea: &mut TextArea<'static>) -> VimTransition {
        if input.key == Key::Null {
            return VimTransition::Nop;
        }

        match self.mode {
            VimMode::Normal => self.transition_normal(input, textarea),
            VimMode::Visual | VimMode::VisualLine => self.transition_visual(input, textarea),
            VimMode::Operator(op) => self.transition_operator(op, input, textarea),
            VimMode::Insert => Self::transition_insert(input, textarea),
            VimMode::Replace(once) => Self::transition_replace(once, input, textarea),
        }
    }

    fn apply_simple_motion(input: &Input, textarea: &mut TextArea<'static>) -> bool {
        match *input {
            Input {
                key: Key::Char('h') | Key::Left,
                ..
            } => {
                textarea.move_cursor(CursorMove::Back);
                true
            }
            Input {
                key: Key::Char('j') | Key::Down,
                ..
            } => {
                textarea.move_cursor(CursorMove::Down);
                true
            }
            Input {
                key: Key::Char('k') | Key::Up,
                ..
            } => {
                textarea.move_cursor(CursorMove::Up);
                true
            }
            Input {
                key: Key::Char('l') | Key::Right,
                ..
            } => {
                textarea.move_cursor(CursorMove::Forward);
                true
            }
            Input {
                key: Key::Char('w'), ..
            } => {
                textarea.move_cursor(CursorMove::WordForward);
                true
            }
            Input {
                key: Key::Char('e'),
                ctrl: false,
                ..
            } => {
                textarea.move_cursor(CursorMove::WordEnd);
                true
            }
            Input {
                key: Key::Char('b'),
                ctrl: false,
                ..
            } => {
                textarea.move_cursor(CursorMove::WordBack);
                true
            }
            _ => false,
        }
    }

    fn apply_boundary_motion(input: &Input, textarea: &mut TextArea<'static>, pending: &mut Option<Input>) -> bool {
        match *input {
            Input {
                key: Key::Char('^') | Key::Char('0'),
                ..
            } => {
                textarea.move_cursor(CursorMove::Head);
                true
            }
            Input {
                key: Key::Char('$'), ..
            } => {
                textarea.move_cursor(CursorMove::End);
                true
            }
            Input {
                key: Key::Char('g'),
                ctrl: false,
                ..
            } if matches!(
                pending,
                Some(Input {
                    key: Key::Char('g'),
                    ctrl: false,
                    ..
                })
            ) =>
            {
                textarea.move_cursor(CursorMove::Top);
                textarea.move_cursor(CursorMove::Head);
                *pending = None;
                true
            }
            Input {
                key: Key::Char('G'),
                ctrl: false,
                ..
            } => {
                textarea.move_cursor(CursorMove::Bottom);
                textarea.move_cursor(CursorMove::Head);
                true
            }
            _ => false,
        }
    }

    fn transition_insert_triggers(input: &Input, textarea: &mut TextArea<'static>) -> Option<VimTransition> {
        match *input {
            Input {
                key: Key::Char('i'), ..
            } => {
                textarea.cancel_selection();
                Some(VimTransition::Mode(VimMode::Insert))
            }
            Input {
                key: Key::Char('I'), ..
            } => {
                textarea.cancel_selection();
                textarea.move_cursor(CursorMove::Head);
                Some(VimTransition::Mode(VimMode::Insert))
            }
            Input {
                key: Key::Char('a'), ..
            } => {
                textarea.cancel_selection();
                if Self::is_before_line_end(textarea) {
                    textarea.move_cursor(CursorMove::Forward);
                }
                Some(VimTransition::Mode(VimMode::Insert))
            }
            Input {
                key: Key::Char('A'), ..
            } => {
                textarea.cancel_selection();
                textarea.move_cursor(CursorMove::End);
                Some(VimTransition::Mode(VimMode::Insert))
            }
            Input {
                key: Key::Char('o'), ..
            } => {
                textarea.move_cursor(CursorMove::End);
                textarea.insert_newline();
                Some(VimTransition::Mode(VimMode::Insert))
            }
            Input {
                key: Key::Char('O'), ..
            } => {
                textarea.move_cursor(CursorMove::Head);
                textarea.insert_newline();
                textarea.move_cursor(CursorMove::Up);
                Some(VimTransition::Mode(VimMode::Insert))
            }
            Input {
                key: Key::Char('s'),
                ctrl: false,
                ..
            } => {
                textarea.delete_next_char();
                Some(VimTransition::Mode(VimMode::Insert))
            }
            Input {
                key: Key::Char('S'),
                ctrl: false,
                ..
            } => {
                textarea.move_cursor(CursorMove::Head);
                textarea.delete_line_by_end();
                Some(VimTransition::Mode(VimMode::Insert))
            }
            Input {
                key: Key::Char('C'), ..
            } => {
                textarea.delete_line_by_end();
                textarea.cancel_selection();
                Some(VimTransition::Mode(VimMode::Insert))
            }
            _ => None,
        }
    }

    fn transition_normal_edit(input: &Input, textarea: &mut TextArea<'static>) -> Option<VimTransition> {
        match *input {
            Input {
                key: Key::Char('x'), ..
            } => {
                textarea.delete_next_char();
                Some(VimTransition::Mode(VimMode::Normal))
            }
            Input {
                key: Key::Char('D'), ..
            } => {
                textarea.delete_line_by_end();
                Some(VimTransition::Mode(VimMode::Normal))
            }
            Input {
                key: Key::Char('u'),
                ctrl: false,
                ..
            } => {
                textarea.undo();
                Some(VimTransition::Mode(VimMode::Normal))
            }
            Input {
                key: Key::Char('r'),
                ctrl: true,
                ..
            } => {
                textarea.redo();
                Some(VimTransition::Mode(VimMode::Normal))
            }
            Input {
                key: Key::Char('p'), ..
            } => {
                if let Ok(Some(text)) = clipboard::get_text() {
                    textarea.set_yank_text(&text);
                }
                textarea.paste();
                Some(VimTransition::Mode(VimMode::Normal))
            }
            Input {
                key: Key::Char('P'), ..
            } => {
                if let Ok(Some(text)) = clipboard::get_text() {
                    textarea.set_yank_text(&text);
                }
                textarea.move_cursor(CursorMove::Back);
                textarea.paste();
                Some(VimTransition::Mode(VimMode::Normal))
            }
            _ => None,
        }
    }

    fn transition_normal(&mut self, input: Input, textarea: &mut TextArea<'static>) -> VimTransition {
        if Self::apply_simple_motion(&input, textarea)
            || Self::apply_boundary_motion(&input, textarea, &mut self.pending)
        {
            return VimTransition::Nop;
        }
        if let Some(tr) = Self::transition_insert_triggers(&input, textarea) {
            return tr;
        }
        if let Some(tr) = Self::transition_normal_edit(&input, textarea) {
            return tr;
        }
        match input {
            Input {
                key: Key::Char('r'),
                ctrl: false,
                ..
            } => VimTransition::Mode(VimMode::Replace(true)),
            Input {
                key: Key::Char('R'),
                ctrl: false,
                ..
            } => VimTransition::Mode(VimMode::Replace(false)),
            Input {
                key: Key::Char('v'),
                ctrl: false,
                ..
            } => {
                textarea.start_selection();
                VimTransition::Mode(VimMode::Visual)
            }
            Input {
                key: Key::Char('V'),
                ctrl: false,
                ..
            } => {
                textarea.move_cursor(CursorMove::Head);
                textarea.start_selection();
                textarea.move_cursor(CursorMove::End);
                VimTransition::Mode(VimMode::VisualLine)
            }
            Input {
                key: Key::Char(op @ ('y' | 'd' | 'c')),
                ctrl: false,
                ..
            } => {
                textarea.start_selection();
                VimTransition::Mode(VimMode::Operator(op))
            }
            other => VimTransition::Pending(other),
        }
    }

    fn transition_visual(&mut self, input: Input, textarea: &mut TextArea<'static>) -> VimTransition {
        match input {
            Input { key: Key::Esc, .. }
            | Input {
                key: Key::Char('['),
                ctrl: true,
                ..
            }
            | Input {
                key: Key::Char('v'),
                ctrl: false,
                ..
            } => {
                textarea.cancel_selection();
                VimTransition::Mode(VimMode::Normal)
            }
            Input {
                key: Key::Char('y'),
                ctrl: false,
                ..
            } => {
                textarea.move_cursor(CursorMove::Forward);
                textarea.copy();
                let yanked = textarea.yank_text();
                let _ = clipboard::set_text(&yanked);
                textarea.cancel_selection();
                VimTransition::Mode(VimMode::Normal)
            }
            Input {
                key: Key::Char('d'),
                ctrl: false,
                ..
            } => {
                textarea.move_cursor(CursorMove::Forward);
                textarea.cut();
                let cut = textarea.yank_text();
                let _ = clipboard::set_text(&cut);
                textarea.cancel_selection();
                VimTransition::Mode(VimMode::Normal)
            }
            Input {
                key: Key::Char('c'),
                ctrl: false,
                ..
            } => {
                textarea.move_cursor(CursorMove::Forward);
                textarea.cut();
                let cut = textarea.yank_text();
                let _ = clipboard::set_text(&cut);
                textarea.cancel_selection();
                VimTransition::Mode(VimMode::Insert)
            }
            other => {
                let _ = Self::apply_simple_motion(&other, textarea)
                    || Self::apply_boundary_motion(&other, textarea, &mut self.pending);
                VimTransition::Nop
            }
        }
    }

    fn transition_operator(&mut self, op: char, input: Input, textarea: &mut TextArea<'static>) -> VimTransition {
        if matches!(
            input,
            Input { key: Key::Esc, .. }
                | Input {
                    key: Key::Char('['),
                    ctrl: true,
                    ..
                }
        ) {
            textarea.cancel_selection();
            return VimTransition::Mode(VimMode::Normal);
        }

        if matches!(
            input,
            Input {
                key: Key::Char(c),
                ctrl: false,
                ..
            } if c == op
        ) {
            textarea.move_cursor(CursorMove::Head);
            textarea.start_selection();
            let cur = textarea.cursor();
            textarea.move_cursor(CursorMove::Down);
            if cur == textarea.cursor() {
                textarea.move_cursor(CursorMove::End);
            }
        } else {
            let _ = Self::apply_simple_motion(&input, textarea)
                || Self::apply_boundary_motion(&input, textarea, &mut self.pending);
        }

        match op {
            'y' => {
                textarea.copy();
                let yanked = textarea.yank_text();
                let _ = clipboard::set_text(&yanked);
                textarea.cancel_selection();
                VimTransition::Mode(VimMode::Normal)
            }
            'd' => {
                textarea.cut();
                let cut = textarea.yank_text();
                let _ = clipboard::set_text(&cut);
                textarea.cancel_selection();
                VimTransition::Mode(VimMode::Normal)
            }
            'c' => {
                textarea.cut();
                let cut = textarea.yank_text();
                let _ = clipboard::set_text(&cut);
                textarea.cancel_selection();
                VimTransition::Mode(VimMode::Insert)
            }
            _ => VimTransition::Nop,
        }
    }

    fn transition_insert(input: Input, textarea: &mut TextArea<'static>) -> VimTransition {
        match input {
            Input { key: Key::Esc, .. }
            | Input {
                key: Key::Char('c'),
                ctrl: true,
                ..
            }
            | Input {
                key: Key::Char('['),
                ctrl: true,
                ..
            } => VimTransition::Mode(VimMode::Normal),
            other => {
                textarea.input(other);
                VimTransition::Mode(VimMode::Insert)
            }
        }
    }

    fn transition_replace(once: bool, input: Input, textarea: &mut TextArea<'static>) -> VimTransition {
        match input {
            Input { key: Key::Esc, .. }
            | Input {
                key: Key::Char('['),
                ctrl: true,
                ..
            } => VimTransition::Mode(VimMode::Normal),
            Input {
                key: Key::Char(c),
                ctrl: false,
                alt: false,
                ..
            } => {
                let cursor = textarea.cursor();
                if cursor.0 < textarea.lines().len()
                    && (Self::is_before_line_end(textarea) || textarea.lines()[cursor.0].len() == cursor.1)
                {
                    textarea.delete_next_char();
                    textarea.insert_char(c);
                }
                if once {
                    VimTransition::Mode(VimMode::Normal)
                } else {
                    VimTransition::Mode(VimMode::Replace(false))
                }
            }
            _ => VimTransition::Mode(if once { VimMode::Normal } else { VimMode::Replace(false) }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TextAreaEditor {
    textarea: TextArea<'static>,
    editor_mode: EditorMode,
    vim: Vim,
    pastes: PasteStore,
    placeholder: String,
}

impl Default for TextAreaEditor {
    fn default() -> Self {
        Self::new(EditorMode::Default)
    }
}

impl TextAreaEditor {
    pub fn new(mode: EditorMode) -> Self {
        let mut textarea = TextArea::default();
        textarea.set_cursor_line_style(Style::default());
        textarea.set_style(Style::default());
        let initial_vim_mode = if mode == EditorMode::Vim {
            VimMode::Normal
        } else {
            VimMode::Insert
        };
        textarea.set_cursor_style(initial_vim_mode.cursor_style());

        Self {
            textarea,
            editor_mode: mode,
            vim: Vim::new(initial_vim_mode),
            pastes: PasteStore::default(),
            placeholder: String::new(),
        }
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self.textarea.set_placeholder_text(&self.placeholder);
        self.textarea
            .set_placeholder_style(Style::default().fg(Color::DarkGray));
        self
    }

    pub fn set_placeholder(&mut self, placeholder: impl Into<String>) {
        self.placeholder = placeholder.into();
        self.textarea.set_placeholder_text(&self.placeholder);
        self.textarea
            .set_placeholder_style(Style::default().fg(Color::DarkGray));
    }

    pub fn mode(&self) -> EditorMode {
        self.editor_mode
    }

    pub fn set_mode(&mut self, mode: EditorMode) {
        self.editor_mode = mode;
        if mode == EditorMode::Vim {
            self.vim.mode = VimMode::Normal;
            self.textarea.set_cursor_style(VimMode::Normal.cursor_style());
        } else {
            self.textarea
                .set_cursor_style(Style::default().add_modifier(Modifier::REVERSED));
        }
    }

    pub fn vim_mode(&self) -> VimMode {
        self.vim.mode
    }

    pub fn mode_label(&self) -> &'static str {
        match self.editor_mode {
            EditorMode::Default => "",
            EditorMode::Vim => self.vim.mode.label(),
        }
    }

    pub fn textarea(&self) -> &TextArea<'static> {
        &self.textarea
    }

    pub fn textarea_mut(&mut self) -> &mut TextArea<'static> {
        &mut self.textarea
    }

    pub fn pastes(&self) -> &PasteStore {
        &self.pastes
    }

    pub fn pastes_mut(&mut self) -> &mut PasteStore {
        &mut self.pastes
    }

    pub fn lines(&self) -> &[String] {
        self.textarea.lines()
    }

    pub fn expanded_text(&self) -> String {
        self.pastes.expand(&self.text())
    }

    pub fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    pub fn set_text(&mut self, text: &str) {
        let lines: Vec<String> = text.split('\n').map(String::from).collect();
        self.textarea = TextArea::from(lines);
        self.textarea.move_cursor(CursorMove::Bottom);
        self.textarea.move_cursor(CursorMove::End);
        self.pastes.sync_with_text(text);
        if !self.placeholder.is_empty() {
            self.textarea.set_placeholder_text(&self.placeholder);
            self.textarea
                .set_placeholder_style(Style::default().fg(Color::DarkGray));
        }
        if self.editor_mode == EditorMode::Vim {
            self.textarea.set_cursor_style(self.vim.mode.cursor_style());
        }
    }

    pub fn cursor(&self) -> (usize, usize) {
        let cur = self.textarea.cursor();
        (cur.0, cur.1)
    }

    pub fn is_empty(&self) -> bool {
        let lines = self.textarea.lines();
        lines.is_empty() || (lines.len() == 1 && lines[0].is_empty())
    }

    pub fn byte_cursor(&self) -> usize {
        let cur = self.textarea.cursor();
        let (r, c) = (cur.0, cur.1);
        let lines = self.textarea.lines();
        let mut byte_idx = 0;
        for (i, line) in lines.iter().enumerate() {
            if i < r {
                byte_idx += line.len() + 1;
            } else if i == r {
                let char_offset: usize = line.chars().take(c).map(|ch| ch.len_utf8()).sum();
                byte_idx += char_offset;
                break;
            }
        }
        byte_idx
    }

    pub fn insert_char(&mut self, c: char) {
        self.textarea.insert_char(c);
    }

    pub fn insert_newline(&mut self) {
        self.textarea.insert_newline();
    }

    pub fn handle_paste(&mut self, pasted_text: &str) {
        let clean = sanitize_paste(pasted_text);
        if clean.is_empty() {
            return;
        }

        let cur_line = {
            let cur = self.textarea.cursor();
            let (r, c) = (cur.0, cur.1);
            self.textarea
                .lines()
                .get(r)
                .and_then(|line| line.chars().take(c).last())
        };

        if (clean.starts_with('/') || clean.starts_with('~') || clean.starts_with('.'))
            && let Some(ch) = cur_line
            && (ch.is_alphanumeric() || ch == '_')
        {
            self.textarea.insert_char(' ');
        }

        if check_paste_threshold(&clean) {
            let (_, marker) = self.pastes.insert(clean);
            self.textarea.insert_str(&marker);
        } else {
            self.textarea.insert_str(&clean);
        }
    }

    pub fn handle_clipboard_image(&mut self, path: &Path) {
        let marker = format!("[image {}]", path.display());
        self.textarea.insert_str(&marker);
    }

    pub fn take_submission(&mut self, kind: QueueKind) -> Option<QueuedMessage> {
        let expanded = self.expanded_text();
        let text = expanded.trim().to_string();
        if text.is_empty() {
            return None;
        }
        self.clear();
        Some(QueuedMessage { text, kind })
    }

    pub fn insert(&mut self, text: impl AsRef<str>) {
        self.textarea.insert_str(text.as_ref());
    }

    pub fn backspace(&mut self) -> bool {
        self.textarea.delete_char()
    }

    pub fn delete(&mut self) -> bool {
        self.textarea.delete_next_char()
    }

    pub fn move_left(&mut self) {
        self.textarea.move_cursor(CursorMove::Back);
    }

    pub fn move_right(&mut self) {
        self.textarea.move_cursor(CursorMove::Forward);
    }

    pub fn move_word_left(&mut self) {
        self.textarea.move_cursor(CursorMove::WordBack);
    }

    pub fn move_word_right(&mut self) {
        self.textarea.move_cursor(CursorMove::WordForward);
    }

    pub fn move_to_start(&mut self) {
        self.textarea.move_cursor(CursorMove::Head);
    }

    pub fn move_to_end(&mut self) {
        self.textarea.move_cursor(CursorMove::End);
    }

    pub fn delete_word_backward(&mut self) -> bool {
        self.textarea.delete_word()
    }

    pub fn delete_word_forward(&mut self) -> bool {
        self.textarea.delete_next_word()
    }

    pub fn delete_to_line_start(&mut self) -> bool {
        self.textarea.delete_line_by_head()
    }

    pub fn delete_to_line_end(&mut self) -> bool {
        self.textarea.delete_line_by_end()
    }

    pub fn yank(&mut self) {
        self.textarea.paste();
    }

    pub fn undo(&mut self) -> bool {
        self.textarea.undo()
    }

    pub fn redo(&mut self) -> bool {
        self.textarea.redo()
    }

    pub fn clear(&mut self) {
        self.textarea = TextArea::default();
        if !self.placeholder.is_empty() {
            self.textarea.set_placeholder_text(&self.placeholder);
            self.textarea
                .set_placeholder_style(Style::default().fg(Color::DarkGray));
        }
        self.pastes.clear();
        if self.editor_mode == EditorMode::Vim {
            self.vim.mode = VimMode::Normal;
            self.textarea.set_cursor_style(VimMode::Normal.cursor_style());
        }
    }
}

impl PartialEq for TextAreaEditor {
    fn eq(&self, other: &Self) -> bool {
        self.text() == other.text() && self.editor_mode == other.editor_mode && self.pastes == other.pastes
    }
}

impl Eq for TextAreaEditor {}

impl PromptEditor for TextAreaEditor {
    fn text(&self) -> String {
        self.text()
    }

    fn set_text(&mut self, text: &str) {
        self.set_text(text);
    }

    fn is_empty(&self) -> bool {
        self.is_empty()
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if self.editor_mode == EditorMode::Vim {
            if self.vim.mode == VimMode::Normal && key.code == KeyCode::Enter && key.modifiers.is_empty() {
                self.textarea.move_cursor(CursorMove::Down);
                return true;
            }
            if self.vim.mode == VimMode::Insert && key.code == KeyCode::Enter && key.modifiers.is_empty() {
                return false;
            }

            let input: Input = key.into();
            let transition = self.vim.transition(input, &mut self.textarea);
            match transition {
                VimTransition::Mode(mode) => {
                    self.vim.mode = mode;
                    self.textarea.set_cursor_style(mode.cursor_style());
                    self.vim.pending = None;
                    true
                }
                VimTransition::Pending(p) => {
                    self.vim.pending = Some(p);
                    true
                }
                VimTransition::Nop => true,
            }
        } else {
            if key.code == KeyCode::Enter && key.modifiers.is_empty() {
                return false;
            }
            if key.code == KeyCode::Enter
                && (key.modifiers.contains(KeyModifiers::SHIFT) || key.modifiers.contains(KeyModifiers::CONTROL))
            {
                self.textarea.insert_newline();
                return true;
            }
            if key.code == KeyCode::Char('j') && key.modifiers.contains(KeyModifiers::CONTROL) {
                self.textarea.insert_newline();
                return true;
            }
            self.textarea.input(key);
            true
        }
    }

    fn insert_str(&mut self, text: &str) {
        self.textarea.insert_str(text);
    }

    fn clear(&mut self) {
        self.clear();
    }

    fn cursor(&self) -> (usize, usize) {
        self.cursor()
    }
}

impl TerminalComponent for TextAreaEditor {
    fn render(&self, frame: &mut ratatui::Frame, area: Rect) {
        frame.render_widget(&self.textarea, area);
        let (row, col) = self.cursor();
        let cursor_x = area.x.saturating_add(col as u16);
        let cursor_y = area.y.saturating_add(row as u16);
        if cursor_x < area.right() && cursor_y < area.bottom() {
            frame.set_cursor_position(Position::new(cursor_x, cursor_y));
        }
    }

    fn desired_height(&self, _width: u16) -> u16 {
        self.textarea.lines().len().max(1) as u16
    }
}
