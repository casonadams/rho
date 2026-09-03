use super::{
    paste::{
        PasteStore, check_paste_threshold, find_marker_covering, find_marker_ending_at, find_marker_starting_at,
        sanitize_paste,
    },
    types::{QueueKind, QueuedMessage},
};
use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone, PartialEq, Eq)]
struct EditorSnapshot {
    text: String,
    cursor: usize,
    pastes: PasteStore,
}

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

    pub fn handle_paste(&mut self, pasted_text: &str) {
        let clean = sanitize_paste(pasted_text);
        if clean.is_empty() {
            return;
        }
        self.record_undo();

        if (clean.starts_with('/') || clean.starts_with('~') || clean.starts_with('.'))
            && let Some((_, ch)) = self.text[..self.cursor].char_indices().next_back()
            && (ch.is_alphanumeric() || ch == '_')
        {
            self.text.insert(self.cursor, ' ');
            self.cursor += 1;
        }

        if check_paste_threshold(&clean) {
            let (_, marker) = self.pastes.insert(clean);
            self.text.insert_str(self.cursor, &marker);
            self.cursor += marker.len();
        } else {
            self.text.insert_str(self.cursor, &clean);
            self.cursor += clean.len();
        }
        self.preferred_column = None;
    }

    pub fn insert(&mut self, value: char) {
        self.record_undo();
        self.text.insert(self.cursor, value);
        self.cursor += value.len_utf8();
        self.preferred_column = None;
    }

    pub fn insert_newline(&mut self) {
        self.insert('\n');
    }

    pub fn backspace(&mut self) {
        if let Some(marker) =
            find_marker_ending_at(&self.text, self.cursor).or_else(|| find_marker_covering(&self.text, self.cursor))
        {
            self.record_undo();
            self.text.drain(marker.start..marker.end);
            self.cursor = marker.start;
            self.pastes.remove_and_renumber(marker.id, &mut self.text);
            self.preferred_column = None;
            return;
        }
        let Some((index, _)) = self.text[..self.cursor].char_indices().next_back() else {
            return;
        };
        self.record_undo();
        self.text.drain(index..self.cursor);
        self.cursor = index;
        self.preferred_column = None;
    }

    pub fn delete(&mut self) {
        if let Some(marker) =
            find_marker_starting_at(&self.text, self.cursor).or_else(|| find_marker_covering(&self.text, self.cursor))
        {
            self.record_undo();
            self.text.drain(marker.start..marker.end);
            self.pastes.remove_and_renumber(marker.id, &mut self.text);
            self.preferred_column = None;
            return;
        }
        let Some(character) = self.text[self.cursor..].chars().next() else {
            return;
        };
        self.record_undo();
        self.text.drain(self.cursor..self.cursor + character.len_utf8());
        self.preferred_column = None;
    }

    pub fn move_left(&mut self) {
        if let Some(marker) = find_marker_ending_at(&self.text, self.cursor) {
            self.cursor = marker.start;
        } else if let Some(marker) = find_marker_covering(&self.text, self.cursor) {
            self.cursor = marker.start;
        } else if let Some((index, _)) = self.text[..self.cursor].char_indices().next_back() {
            self.cursor = index;
        }
        self.preferred_column = None;
    }

    pub fn move_right(&mut self) {
        if let Some(marker) = find_marker_starting_at(&self.text, self.cursor) {
            self.cursor = marker.end;
        } else if let Some(marker) = find_marker_covering(&self.text, self.cursor) {
            self.cursor = marker.end;
        } else if let Some(character) = self.text[self.cursor..].chars().next() {
            self.cursor += character.len_utf8();
        }
        self.preferred_column = None;
    }

    pub fn move_word_left(&mut self) {
        let slice = &self.text[..self.cursor];
        let mut chars = slice.char_indices().rev().peekable();
        while let Some((_, c)) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
            } else {
                break;
            }
        }
        let mut new_cursor = 0;
        let mut is_alphanumeric = None;
        while let Some((idx, c)) = chars.peek() {
            if c.is_whitespace() {
                break;
            }
            let is_an = c.is_alphanumeric() || *c == '_';
            if let Some(prev) = is_alphanumeric {
                if prev != is_an {
                    break;
                }
            } else {
                is_alphanumeric = Some(is_an);
            }
            new_cursor = *idx;
            chars.next();
        }
        self.cursor = new_cursor;
        if let Some(marker) = find_marker_covering(&self.text, self.cursor) {
            self.cursor = marker.start;
        }
        self.preferred_column = None;
    }

    pub fn move_word_right(&mut self) {
        let slice = &self.text[self.cursor..];
        let mut chars = slice.char_indices().peekable();
        while let Some((_, c)) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
            } else {
                break;
            }
        }
        let mut is_alphanumeric = None;
        let mut offset = slice.len();
        while let Some((idx, c)) = chars.peek() {
            if c.is_whitespace() {
                offset = *idx;
                break;
            }
            let is_an = c.is_alphanumeric() || *c == '_';
            if let Some(prev) = is_alphanumeric {
                if prev != is_an {
                    offset = *idx;
                    break;
                }
            } else {
                is_alphanumeric = Some(is_an);
            }
            chars.next();
        }
        self.cursor += offset;
        if let Some(marker) = find_marker_covering(&self.text, self.cursor) {
            self.cursor = marker.end;
        }
        self.preferred_column = None;
    }

    pub fn delete_word_backward(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.record_undo();
        let old_cursor = self.cursor;
        self.move_word_left();
        let new_cursor = self.cursor;
        self.cursor = old_cursor;
        let killed: String = self.text.drain(new_cursor..old_cursor).collect();
        if !killed.is_empty() {
            self.kill_ring.push(killed);
        }
        self.cursor = new_cursor;
        self.pastes.sync_with_text(&self.text);
        self.preferred_column = None;
    }

    pub fn delete_word_forward(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        self.record_undo();
        let old_cursor = self.cursor;
        self.move_word_right();
        let new_cursor = self.cursor;
        self.cursor = old_cursor;
        let killed: String = self.text.drain(old_cursor..new_cursor).collect();
        if !killed.is_empty() {
            self.kill_ring.push(killed);
        }
        self.pastes.sync_with_text(&self.text);
        self.preferred_column = None;
    }

    pub fn delete_to_line_start(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.record_undo();
        let line_start = self.text[..self.cursor].rfind('\n').map(|idx| idx + 1).unwrap_or(0);
        let killed: String = self.text.drain(line_start..self.cursor).collect();
        if !killed.is_empty() {
            self.kill_ring.push(killed);
        }
        self.cursor = line_start;
        self.pastes.sync_with_text(&self.text);
        self.preferred_column = None;
    }

    pub fn delete_to_line_end(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        self.record_undo();
        let line_end = self.text[self.cursor..]
            .find('\n')
            .map(|idx| self.cursor + idx)
            .unwrap_or(self.text.len());
        let line_end = if line_end == self.cursor && line_end < self.text.len() {
            line_end + 1
        } else {
            line_end
        };
        let killed: String = self.text.drain(self.cursor..line_end).collect();
        if !killed.is_empty() {
            self.kill_ring.push(killed);
        }
        self.pastes.sync_with_text(&self.text);
        self.preferred_column = None;
    }

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

    fn record_undo(&mut self) {
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
            if let Some(marker) = find_marker_covering(&self.text, self.cursor) {
                let to_start = self.cursor - marker.start;
                let to_end = marker.end - self.cursor;
                self.cursor = if to_start <= to_end { marker.start } else { marker.end };
            }
            self.preferred_column = Some(preferred_column);
            true
        } else {
            false
        }
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

pub(crate) fn editor_boundaries(text: &str) -> impl Iterator<Item = usize> + '_ {
    std::iter::once(0).chain(
        text.char_indices()
            .map(|(index, character)| index + character.len_utf8()),
    )
}

pub(crate) fn editor_cursor_position(text: &str, cursor: usize, terminal_width: usize) -> (usize, usize) {
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
