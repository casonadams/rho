use crate::ui::editor::TextAreaEditor;
use crate::ui::interactive::state::{
    paste::{
        PasteStore, check_paste_threshold, find_marker_covering, find_marker_ending_at, find_marker_starting_at,
        sanitize_paste,
    },
    types::{QueueKind, QueuedMessage},
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct EditorSnapshot {
    text: String,
    cursor: usize,
    pastes: PasteStore,
}

#[derive(Debug, Clone, Default)]
pub struct EditorState {
    inner: TextAreaEditor,
    cached_text: String,
    preferred_column: Option<usize>,
    kill_ring: Vec<String>,
    undo_stack: Vec<EditorSnapshot>,
}

impl PartialEq for EditorState {
    fn eq(&self, other: &Self) -> bool {
        self.cached_text == other.cached_text && self.inner.pastes() == other.inner.pastes()
    }
}

impl Eq for EditorState {}

fn char_class(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl EditorState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn text(&self) -> &str {
        &self.cached_text
    }

    pub fn cursor(&self) -> usize {
        self.inner.byte_cursor()
    }

    pub fn cursor_coords(&self) -> (usize, usize) {
        self.inner.cursor()
    }

    pub fn is_empty(&self) -> bool {
        self.cached_text.is_empty()
    }

    pub fn pastes(&self) -> &PasteStore {
        self.inner.pastes()
    }

    pub fn pastes_mut(&mut self) -> &mut PasteStore {
        self.inner.pastes_mut()
    }

    pub fn expanded_text(&self) -> String {
        self.inner.expanded_text()
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        let t = text.into();
        self.inner.set_text(&t);
        self.cached_text = t;
        self.preferred_column = None;
    }

    pub fn set_byte_cursor(&mut self, target_byte: usize) {
        let mut current = 0;
        let lines = self.inner.textarea().lines();
        for (r, line) in lines.iter().enumerate() {
            let line_len = line.len();
            if current + line_len >= target_byte {
                let offset_in_line = target_byte.saturating_sub(current);
                let col = line[..offset_in_line.min(line_len)].chars().count();
                self.inner
                    .textarea_mut()
                    .move_cursor(ratatui_textarea::CursorMove::Jump(r as u16, col as u16));
                return;
            }
            current += line_len + 1;
        }
        self.inner
            .textarea_mut()
            .move_cursor(ratatui_textarea::CursorMove::Bottom);
        self.inner.textarea_mut().move_cursor(ratatui_textarea::CursorMove::End);
    }

    pub fn take_submission(&mut self, kind: QueueKind) -> Option<QueuedMessage> {
        let expanded = self.expanded_text();
        let text = expanded.trim().to_string();
        if text.is_empty() {
            return None;
        }
        self.cached_text.clear();
        self.inner.clear();
        self.preferred_column = None;
        Some(QueuedMessage { text, kind })
    }

    pub fn record_undo(&mut self) {
        let cursor = self.cursor();
        let pastes = self.inner.pastes().clone();
        if self
            .undo_stack
            .last()
            .map(|s| s.text != self.cached_text || s.cursor != cursor || s.pastes != pastes)
            .unwrap_or(true)
        {
            if self.undo_stack.len() >= 50 {
                self.undo_stack.remove(0);
            }
            self.undo_stack.push(EditorSnapshot {
                text: self.cached_text.clone(),
                cursor,
                pastes,
            });
        }
    }

    pub fn insert(&mut self, value: char) {
        self.record_undo();
        let cursor = self.cursor();
        self.cached_text.insert(cursor, value);
        let new_cursor = cursor + value.len_utf8();
        self.inner.set_text(&self.cached_text);
        self.set_byte_cursor(new_cursor);
        self.preferred_column = None;
    }

    pub fn insert_newline(&mut self) {
        self.insert('\n');
    }

    pub fn handle_paste(&mut self, pasted_text: &str) {
        let clean = sanitize_paste(pasted_text);
        if clean.is_empty() {
            return;
        }
        self.record_undo();

        let cursor = self.cursor();
        if (clean.starts_with('/') || clean.starts_with('~') || clean.starts_with('.'))
            && let Some((_, ch)) = self.cached_text[..cursor].char_indices().next_back()
            && (ch.is_alphanumeric() || ch == '_')
        {
            self.cached_text.insert(cursor, ' ');
            self.inner.set_text(&self.cached_text);
            self.set_byte_cursor(cursor + 1);
        }

        let cursor = self.cursor();
        if check_paste_threshold(&clean) {
            let (_, marker) = self.inner.pastes_mut().insert(clean);
            self.cached_text.insert_str(cursor, &marker);
            let new_cursor = cursor + marker.len();
            self.inner.set_text(&self.cached_text);
            self.set_byte_cursor(new_cursor);
        } else {
            self.cached_text.insert_str(cursor, &clean);
            let new_cursor = cursor + clean.len();
            self.inner.set_text(&self.cached_text);
            self.set_byte_cursor(new_cursor);
        }
        self.preferred_column = None;
    }

    pub fn backspace(&mut self) {
        let cursor = self.cursor();
        if let Some(marker) =
            find_marker_ending_at(&self.cached_text, cursor).or_else(|| find_marker_covering(&self.cached_text, cursor))
        {
            self.record_undo();
            self.cached_text.drain(marker.start..marker.end);
            self.inner
                .pastes_mut()
                .remove_and_renumber(marker.id, &mut self.cached_text);
            self.inner.set_text(&self.cached_text);
            self.set_byte_cursor(marker.start);
            self.preferred_column = None;
            return;
        }
        let Some((index, _)) = self.cached_text[..cursor].char_indices().next_back() else {
            return;
        };
        self.record_undo();
        self.cached_text.drain(index..cursor);
        self.inner.set_text(&self.cached_text);
        self.set_byte_cursor(index);
        self.preferred_column = None;
    }

    pub fn delete(&mut self) {
        let cursor = self.cursor();
        if let Some(marker) = find_marker_starting_at(&self.cached_text, cursor)
            .or_else(|| find_marker_covering(&self.cached_text, cursor))
        {
            self.record_undo();
            self.cached_text.drain(marker.start..marker.end);
            self.inner
                .pastes_mut()
                .remove_and_renumber(marker.id, &mut self.cached_text);
            self.inner.set_text(&self.cached_text);
            self.set_byte_cursor(marker.start);
            self.preferred_column = None;
            return;
        }
        let Some(ch) = self.cached_text[cursor..].chars().next() else {
            return;
        };
        self.record_undo();
        self.cached_text.drain(cursor..cursor + ch.len_utf8());
        self.inner.set_text(&self.cached_text);
        self.set_byte_cursor(cursor);
        self.preferred_column = None;
    }

    pub fn move_left(&mut self) {
        let cursor = self.cursor();
        if let Some(marker) = find_marker_ending_at(&self.cached_text, cursor) {
            self.set_byte_cursor(marker.start);
            self.preferred_column = None;
            return;
        }
        if let Some(marker) = find_marker_covering(&self.cached_text, cursor) {
            self.set_byte_cursor(marker.start);
            self.preferred_column = None;
            return;
        }
        self.inner
            .textarea_mut()
            .move_cursor(ratatui_textarea::CursorMove::Back);
        self.preferred_column = None;
    }

    pub fn move_right(&mut self) {
        let cursor = self.cursor();
        if let Some(marker) = find_marker_starting_at(&self.cached_text, cursor) {
            self.set_byte_cursor(marker.end);
            self.preferred_column = None;
            return;
        }
        if let Some(marker) = find_marker_covering(&self.cached_text, cursor) {
            self.set_byte_cursor(marker.end);
            self.preferred_column = None;
            return;
        }
        self.inner
            .textarea_mut()
            .move_cursor(ratatui_textarea::CursorMove::Forward);
        self.preferred_column = None;
    }

    pub fn move_word_left(&mut self) {
        let cursor = self.cursor();
        if cursor == 0 {
            return;
        }
        let slice = &self.cached_text[..cursor];
        let mut chars = slice.char_indices().rev().peekable();
        while let Some((_, c)) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
            } else {
                break;
            }
        }
        let mut new_cursor = 0;
        let mut is_an = None;
        while let Some((idx, c)) = chars.peek() {
            if c.is_whitespace() {
                break;
            }
            let an = char_class(*c);
            if let Some(prev) = is_an {
                if prev != an {
                    break;
                }
            } else {
                is_an = Some(an);
            }
            new_cursor = *idx;
            chars.next();
        }
        if let Some(marker) = find_marker_covering(&self.cached_text, new_cursor) {
            new_cursor = marker.start;
        }
        self.set_byte_cursor(new_cursor);
        self.preferred_column = None;
    }

    pub fn move_word_right(&mut self) {
        let cursor = self.cursor();
        if cursor >= self.cached_text.len() {
            return;
        }
        let slice = &self.cached_text[cursor..];
        let mut chars = slice.char_indices().peekable();
        while let Some((_, c)) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
            } else {
                break;
            }
        }
        let mut offset = slice.len();
        let mut is_an = None;
        while let Some((idx, c)) = chars.peek() {
            if c.is_whitespace() {
                offset = *idx;
                break;
            }
            let an = char_class(*c);
            if let Some(prev) = is_an {
                if prev != an {
                    offset = *idx;
                    break;
                }
            } else {
                is_an = Some(an);
            }
            chars.next();
        }
        let mut new_cursor = cursor + offset;
        if let Some(marker) = find_marker_covering(&self.cached_text, new_cursor) {
            new_cursor = marker.end;
        }
        self.set_byte_cursor(new_cursor);
        self.preferred_column = None;
    }

    pub fn move_to_start(&mut self) {
        self.set_byte_cursor(0);
        self.preferred_column = None;
    }

    pub fn move_to_end(&mut self) {
        self.set_byte_cursor(self.cached_text.len());
        self.preferred_column = None;
    }

    fn visual_lines(&self, width: usize) -> (Vec<(usize, usize)>, usize) {
        let width = width.max(1);
        let byte_cursor = self.cursor();
        let mut lines = Vec::new();
        let mut current_start = 0;
        let mut current_width = 0;
        let mut cursor_row = 0;

        for (idx, ch) in self.cached_text.char_indices() {
            if idx == byte_cursor {
                cursor_row = lines.len();
            }
            if ch == '\n' {
                lines.push((current_start, idx));
                current_start = idx + 1;
                current_width = 0;
                continue;
            }
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if current_width > 0 && current_width + cw > width {
                lines.push((current_start, idx));
                current_start = idx;
                current_width = 0;
            }
            current_width += cw;
        }
        if byte_cursor >= current_start {
            cursor_row = lines.len();
        }
        lines.push((current_start, self.cached_text.len()));
        (lines, cursor_row)
    }

    pub fn move_up(&mut self, width: usize) -> bool {
        let (lines, cursor_row) = self.visual_lines(width);
        if cursor_row == 0 {
            return false;
        }
        let target_row = cursor_row - 1;
        let (row_start, _row_end) = lines[cursor_row];
        let current_col = self.cursor().saturating_sub(row_start);
        let pref = self.preferred_column.get_or_insert(current_col);
        let (target_start, target_end) = lines[target_row];
        let target_len = target_end.saturating_sub(target_start);
        let target_byte = target_start + (*pref).min(target_len);
        self.set_byte_cursor(target_byte);
        true
    }

    pub fn move_down(&mut self, width: usize) -> bool {
        let (lines, cursor_row) = self.visual_lines(width);
        if cursor_row + 1 >= lines.len() {
            return false;
        }
        let target_row = cursor_row + 1;
        let (row_start, _row_end) = lines[cursor_row];
        let current_col = self.cursor().saturating_sub(row_start);
        let pref = self.preferred_column.get_or_insert(current_col);
        let (target_start, target_end) = lines[target_row];
        let target_len = target_end.saturating_sub(target_start);
        let target_byte = target_start + (*pref).min(target_len);
        self.set_byte_cursor(target_byte);
        true
    }

    fn kill_range(&mut self, range: std::ops::Range<usize>) {
        let killed: String = self.cached_text.drain(range).collect();
        if !killed.is_empty() {
            self.kill_ring.push(killed);
        }
    }

    pub fn delete_word_backward(&mut self) {
        let cursor = self.cursor();
        if cursor == 0 {
            return;
        }
        self.record_undo();
        let old_cursor = cursor;
        self.move_word_left();
        let new_cursor = self.cursor();
        self.kill_range(new_cursor..old_cursor);
        self.inner.pastes_mut().sync_with_text(&self.cached_text);
        self.inner.set_text(&self.cached_text);
        self.set_byte_cursor(new_cursor);
        self.preferred_column = None;
    }

    pub fn delete_word_forward(&mut self) {
        let cursor = self.cursor();
        if cursor >= self.cached_text.len() {
            return;
        }
        self.record_undo();
        let old_cursor = cursor;
        self.move_word_right();
        let new_cursor = self.cursor();
        self.kill_range(old_cursor..new_cursor);
        self.inner.pastes_mut().sync_with_text(&self.cached_text);
        self.inner.set_text(&self.cached_text);
        self.set_byte_cursor(old_cursor);
        self.preferred_column = None;
    }

    pub fn delete_to_line_start(&mut self) {
        let cursor = self.cursor();
        if cursor == 0 {
            return;
        }
        self.record_undo();
        let line_start = self.cached_text[..cursor].rfind('\n').map(|idx| idx + 1).unwrap_or(0);
        self.kill_range(line_start..cursor);
        self.inner.pastes_mut().sync_with_text(&self.cached_text);
        self.inner.set_text(&self.cached_text);
        self.set_byte_cursor(line_start);
        self.preferred_column = None;
    }

    pub fn delete_to_line_end(&mut self) {
        let cursor = self.cursor();
        if cursor >= self.cached_text.len() {
            return;
        }
        self.record_undo();
        let line_end = self.cached_text[cursor..]
            .find('\n')
            .map(|idx| cursor + idx)
            .unwrap_or(self.cached_text.len());
        let line_end = if line_end == cursor && line_end < self.cached_text.len() {
            line_end + 1
        } else {
            line_end
        };
        self.kill_range(cursor..line_end);
        self.inner.pastes_mut().sync_with_text(&self.cached_text);
        self.inner.set_text(&self.cached_text);
        self.set_byte_cursor(cursor);
        self.preferred_column = None;
    }

    pub fn yank(&mut self) {
        if let Some(last) = self.kill_ring.last().cloned() {
            self.record_undo();
            let cursor = self.cursor();
            self.cached_text.insert_str(cursor, &last);
            let new_cursor = cursor + last.len();
            self.inner.pastes_mut().sync_with_text(&self.cached_text);
            self.inner.set_text(&self.cached_text);
            self.set_byte_cursor(new_cursor);
            self.preferred_column = None;
        }
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.cached_text = prev.text;
            *self.inner.pastes_mut() = prev.pastes;
            self.inner.set_text(&self.cached_text);
            self.set_byte_cursor(prev.cursor.min(self.cached_text.len()));
            self.preferred_column = None;
        }
    }

    pub fn inner(&self) -> &TextAreaEditor {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut TextAreaEditor {
        &mut self.inner
    }
}
