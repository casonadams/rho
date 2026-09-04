use super::EditorState;
use super::geometry::{editor_boundaries, editor_cursor_position};
use crate::ui::interactive::state::paste::{find_marker_covering, find_marker_ending_at, find_marker_starting_at};

impl EditorState {
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
}
