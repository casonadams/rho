pub mod cursor;

use cursor::{char_class, compute_vertical_target, skip_whitespace_back, skip_whitespace_fwd, snap_to_marker_end};

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

    pub fn expanded_text(&self) -> String {
        self.pastes.expand(&self.text)
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

    fn kill_range(&mut self, range: std::ops::Range<usize>) {
        let killed: String = self.text.drain(range).collect();
        if !killed.is_empty() {
            self.kill_ring.push(killed);
        }
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
        self.kill_range(new_cursor..old_cursor);
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
        self.kill_range(old_cursor..new_cursor);
        self.pastes.sync_with_text(&self.text);
        self.preferred_column = None;
    }

    pub fn delete_to_line_start(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.record_undo();
        let line_start = self.text[..self.cursor].rfind('\n').map(|idx| idx + 1).unwrap_or(0);
        self.kill_range(line_start..self.cursor);
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
        self.kill_range(self.cursor..line_end);
        self.pastes.sync_with_text(&self.text);
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
        let mut chars = skip_whitespace_back(slice);
        let mut new_cursor = 0;
        let mut is_alphanumeric = None;
        while let Some((idx, c)) = chars.peek() {
            if c.is_whitespace() {
                break;
            }
            let is_an = char_class(*c);
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
        snap_to_marker_end(&self.text, &mut self.cursor, true);
        self.preferred_column = None;
    }

    pub fn move_word_right(&mut self) {
        let slice = &self.text[self.cursor..];
        let mut chars = skip_whitespace_fwd(slice);
        let mut is_alphanumeric = None;
        let mut offset = slice.len();
        while let Some((idx, c)) = chars.peek() {
            if c.is_whitespace() {
                offset = *idx;
                break;
            }
            let is_an = char_class(*c);
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
        snap_to_marker_end(&self.text, &mut self.cursor, false);
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
        if let Some((target_cursor, preferred_column)) = compute_vertical_target(
            &self.text,
            self.cursor,
            terminal_width,
            row_delta,
            self.preferred_column,
        ) {
            self.cursor = target_cursor;
            self.preferred_column = Some(preferred_column);
            true
        } else {
            false
        }
    }
}
