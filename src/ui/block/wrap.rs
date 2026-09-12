use anstyle::Style;
use regex::Regex;
use std::sync::LazyLock;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub(crate) static ANSI_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*m").expect("valid ANSI escape pattern"));

pub(crate) fn visible_width(content: &str) -> usize {
    UnicodeWidthStr::width(ANSI_PATTERN.replace_all(content, "").as_ref())
}

fn skip_color_params(params: &mut std::iter::Peekable<std::str::Split<'_, char>>) {
    match params.peek().copied() {
        Some("5") => {
            params.next();
            params.next();
        }
        Some("2") => {
            params.next();
            params.next();
            params.next();
            params.next();
        }
        _ => {}
    }
}

pub(crate) fn sgr_resets_background(sequence: &str) -> bool {
    let Some(inner) = sequence.strip_prefix("\x1b[").and_then(|s| s.strip_suffix('m')) else {
        return false;
    };
    if inner.is_empty() {
        return true;
    }
    let mut params = inner.split(';').peekable();
    while let Some(param) = params.next() {
        if param.is_empty() || param == "0" || param == "00" || param == "49" {
            return true;
        }
        if param == "38" || param == "48" {
            skip_color_params(&mut params);
        }
    }
    false
}

struct WrapState<'a> {
    lines: &'a mut Vec<String>,
    width: usize,
    bg_code: String,
    current: String,
    active_sgr: String,
    current_width: usize,
    offset: usize,
    pending_spaces: String,
    pending_spaces_width: usize,
    pending_word: String,
    pending_word_width: usize,
}

impl WrapState<'_> {
    fn flush_line(&mut self) {
        self.lines.push(std::mem::take(&mut self.current));
        self.current.push_str(&self.active_sgr);
        self.current_width = 0;
    }

    fn commit_pending_word(&mut self) {
        if self.pending_word.is_empty() && self.pending_word_width == 0 {
            return;
        }
        let needed = self.pending_spaces_width + self.pending_word_width;
        if self.current_width > 0 && self.current_width + needed > self.width {
            self.flush_line();
            self.pending_spaces.clear();
            self.pending_spaces_width = 0;
        }
        if self.current_width > 0 || self.lines.is_empty() {
            self.current.push_str(&self.pending_spaces);
            self.current_width += self.pending_spaces_width;
        }
        self.pending_spaces.clear();
        self.pending_spaces_width = 0;

        self.current.push_str(&self.pending_word);
        self.current_width += self.pending_word_width;
        self.pending_word.clear();
        self.pending_word_width = 0;
    }

    fn push_char(&mut self, character: char, character_width: usize) {
        if character == ' ' || character == '\t' {
            self.commit_pending_word();
            self.pending_spaces.push(character);
            self.pending_spaces_width += character_width;
        } else {
            if self.pending_word_width + character_width > self.width {
                if self.current_width > 0 {
                    self.flush_line();
                    self.pending_spaces.clear();
                    self.pending_spaces_width = 0;
                }
                if self.pending_word_width + character_width > self.width && self.pending_word_width > 0 {
                    self.current.push_str(&self.pending_word);
                    self.flush_line();
                    self.pending_word.clear();
                    self.pending_word_width = 0;
                }
            }
            self.pending_word.push(character);
            self.pending_word_width += character_width;
        }
    }

    fn consume_sgr(&mut self, content: &str) {
        let Some(rel) = content[self.offset..].find('m') else {
            return;
        };
        let end = self.offset + rel + 1;
        let sequence = &content[self.offset..end];
        self.pending_word.push_str(sequence);
        if sgr_resets_background(sequence) {
            if !self.bg_code.is_empty() {
                self.pending_word.push_str(&self.bg_code);
            }
            self.active_sgr.clear();
        } else {
            self.active_sgr.push_str(sequence);
        }
        self.offset = end;
    }
}

pub(crate) fn wrap_styled_line(content: &str, width: usize, bg_style: Style) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut state = WrapState {
        lines: &mut lines,
        width,
        bg_code: bg_style.render().to_string(),
        current: String::new(),
        active_sgr: String::new(),
        current_width: 0,
        offset: 0,
        pending_spaces: String::new(),
        pending_spaces_width: 0,
        pending_word: String::new(),
        pending_word_width: 0,
    };

    while state.offset < content.len() {
        if content.as_bytes()[state.offset..].starts_with(b"\x1b[") {
            state.consume_sgr(content);
            continue;
        }
        let Some(character) = content[state.offset..].chars().next() else {
            break;
        };
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        state.push_char(character, character_width);
        state.offset += character.len_utf8();
    }

    state.commit_pending_word();
    if visible_width(&state.current) > 0 || state.lines.is_empty() {
        state.lines.push(state.current);
    }
    lines
}

pub(crate) fn wrap_plain_text(content: &str, width: usize) -> Vec<String> {
    content
        .split('\n')
        .flat_map(|line| wrap_styled_line(line, width, Style::new()))
        .collect()
}
