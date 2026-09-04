use super::EditorState;
use crate::ui::interactive::state::paste::{
    check_paste_threshold, find_marker_covering, find_marker_ending_at, find_marker_starting_at, sanitize_paste,
};

impl EditorState {
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
}
