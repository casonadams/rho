use crate::ui::block::{apply_pending_ansi, leading_escape};

#[derive(Debug, Default)]
pub(crate) struct WordWrap {
    pub(crate) width: usize,
    pub(crate) current_width: usize,
    pub(crate) pending_spaces: String,
    pub(crate) pending_spaces_width: usize,
    pub(crate) pending_word: String,
    pub(crate) pending_word_width: usize,
}

impl WordWrap {
    pub(crate) fn new(width: usize) -> Self {
        Self {
            width,
            ..Self::default()
        }
    }

    pub(crate) fn push_space(&mut self, character: char, character_width: usize) {
        self.pending_spaces.push(character);
        self.pending_spaces_width += character_width;
    }

    pub(crate) fn push_word_char(&mut self, character: char, character_width: usize) {
        self.pending_word.push(character);
        self.pending_word_width += character_width;
    }

    pub(crate) fn clear_spaces(&mut self) {
        self.pending_spaces.clear();
        self.pending_spaces_width = 0;
    }

    pub(crate) fn break_line(&mut self, current: &mut String, mut finish: impl FnMut(&mut String)) {
        finish(current);
        self.current_width = 0;
    }

    pub(crate) fn commit(&mut self, current: &mut String, keep_leading: bool, mut finish: impl FnMut(&mut String)) {
        if self.pending_word.is_empty() && self.pending_word_width == 0 {
            return;
        }
        let needed = self.pending_spaces_width + self.pending_word_width;
        if self.current_width > 0 && self.current_width + needed > self.width {
            self.break_line(current, &mut finish);
            self.clear_spaces();
        }
        if self.current_width > 0 || keep_leading {
            current.push_str(&self.pending_spaces);
            self.current_width += self.pending_spaces_width;
        }
        self.clear_spaces();
        current.push_str(&self.pending_word);
        self.current_width += self.pending_word_width;
        self.pending_word.clear();
        self.pending_word_width = 0;
    }
}

#[derive(Debug, Default)]
pub(crate) struct AnsiWordWrap {
    pub(crate) word: WordWrap,
    active_ansi: String,
    pending_ansi: String,
}

fn styled_break(line: &mut String, active_ansi: &str, end_line: &mut impl FnMut(&mut String)) {
    if !active_ansi.is_empty() {
        line.push_str("\x1b[0m");
    }
    end_line(line);
    if !active_ansi.is_empty() {
        line.push_str(active_ansi);
    }
}

impl AnsiWordWrap {
    pub(crate) fn new(width: usize) -> Self {
        Self {
            word: WordWrap::new(width),
            ..Self::default()
        }
    }

    pub(crate) fn consume_ansi(&mut self, rest: &str) -> Option<usize> {
        let (seq, is_sgr) = leading_escape(rest)?;
        self.word.pending_word.push_str(seq);
        if is_sgr {
            self.pending_ansi.push_str(seq);
        }
        Some(seq.len())
    }

    pub(crate) fn push_char(
        &mut self,
        current: &mut String,
        character: char,
        character_width: usize,
        mut end_line: impl FnMut(&mut String),
    ) {
        if self.word.pending_word_width + character_width > self.word.width {
            if self.word.current_width > 0 {
                self.break_line(current, &mut end_line);
                self.word.clear_spaces();
            }
            if self.word.pending_word_width + character_width > self.word.width && self.word.pending_word_width > 0 {
                current.push_str(&self.word.pending_word);
                apply_pending_ansi(&mut self.pending_ansi, &mut self.active_ansi);
                self.break_line(current, &mut end_line);
                self.word.pending_word.clear();
                self.word.pending_word_width = 0;
            }
        }
        self.word.push_word_char(character, character_width);
    }

    pub(crate) fn commit(&mut self, current: &mut String, keep_leading: bool, mut end_line: impl FnMut(&mut String)) {
        let active = &self.active_ansi;
        self.word
            .commit(current, keep_leading, |line| styled_break(line, active, &mut end_line));
        apply_pending_ansi(&mut self.pending_ansi, &mut self.active_ansi);
    }

    pub(crate) fn clear_ansi(&mut self) {
        self.pending_ansi.clear();
        self.active_ansi.clear();
    }

    fn break_line(&mut self, current: &mut String, end_line: &mut impl FnMut(&mut String)) {
        let active = &self.active_ansi;
        self.word
            .break_line(current, |line| styled_break(line, active, end_line));
    }
}

pub(crate) fn push_line_into(lines: &mut Vec<String>) -> impl FnMut(&mut String) + '_ {
    move |line: &mut String| lines.push(std::mem::take(line))
}
