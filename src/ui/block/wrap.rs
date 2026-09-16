use crate::ui::theme::Style;
use regex::Regex;
use std::sync::LazyLock;
use unicode_width::UnicodeWidthChar;

pub use rho_ui_core::text::{truncate_to_width, visible_width};

pub static ANSI_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*m").expect("valid ANSI escape pattern"));

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
        if character == '\r' {
            state.offset += 1;
            continue;
        }
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
        .lines()
        .flat_map(|line| wrap_styled_line(line, width, Style::new()))
        .collect()
}

struct LineWrapper<'a> {
    line: &'a str,
    offset: usize,
    max_width: usize,
    current_line: String,
    current_width: usize,
    active_ansi: String,
    pending_ansi: String,
    pending_spaces: String,
    pending_spaces_width: usize,
    pending_word: String,
    pending_word_width: usize,
}

impl<'a> LineWrapper<'a> {
    fn new(line: &'a str, max_width: usize) -> Self {
        Self {
            line,
            offset: 0,
            max_width,
            current_line: String::new(),
            current_width: 0,
            active_ansi: String::new(),
            pending_ansi: String::new(),
            pending_spaces: String::new(),
            pending_spaces_width: 0,
            pending_word: String::new(),
            pending_word_width: 0,
        }
    }

    fn try_consume_ansi(&mut self) -> bool {
        if self.line[self.offset..].starts_with('\x1b')
            && let Some(end) = self.line[self.offset..].find('m')
        {
            let seq = &self.line[self.offset..=self.offset + end];
            self.pending_ansi.push_str(seq);
            self.pending_word.push_str(seq);
            self.offset += end + 1;
            return true;
        }
        false
    }

    fn apply_pending_ansi(&mut self) {
        if self.pending_ansi.is_empty() {
            return;
        }
        let mut rem = self.pending_ansi.as_str();
        while let Some(start) = rem.find('\x1b') {
            if let Some(end) = rem[start..].find('m') {
                let seq = &rem[start..=start + end];
                if seq == "\x1b[0m" || seq == "\x1b[m" {
                    self.active_ansi.clear();
                } else {
                    self.active_ansi.push_str(seq);
                }
                rem = &rem[start + end + 1..];
            } else {
                break;
            }
        }
        self.pending_ansi.clear();
    }

    fn flush_current_line(&mut self, output: &mut Vec<String>) {
        if !self.active_ansi.is_empty() {
            self.current_line.push_str("\x1b[0m");
        }
        output.push(std::mem::take(&mut self.current_line));
        if !self.active_ansi.is_empty() {
            self.current_line.push_str(&self.active_ansi);
        }
        self.current_width = 0;
    }

    fn commit_pending_word(&mut self, output: &mut Vec<String>) {
        if self.pending_word.is_empty() && self.pending_word_width == 0 {
            return;
        }
        let needed = self.pending_spaces_width + self.pending_word_width;
        if self.current_width > 0 && self.current_width + needed > self.max_width {
            self.flush_current_line(output);
            self.pending_spaces.clear();
            self.pending_spaces_width = 0;
        }
        if self.current_width > 0 || output.is_empty() {
            self.current_line.push_str(&self.pending_spaces);
            self.current_width += self.pending_spaces_width;
        }
        self.pending_spaces.clear();
        self.pending_spaces_width = 0;

        self.current_line.push_str(&self.pending_word);
        self.current_width += self.pending_word_width;
        self.pending_word.clear();
        self.pending_word_width = 0;
        self.apply_pending_ansi();
    }

    fn wrap(mut self, output: &mut Vec<String>) {
        while self.offset < self.line.len() {
            if self.try_consume_ansi() {
                continue;
            }
            let Some(c) = self.line[self.offset..].chars().next() else {
                break;
            };
            self.offset += c.len_utf8();
            if c == '\r' {
                continue;
            }

            if c == ' ' || c == '\t' {
                self.commit_pending_word(output);
                let cw = UnicodeWidthChar::width(c).unwrap_or(0);
                self.pending_spaces.push(c);
                self.pending_spaces_width += cw;
            } else {
                let cw = UnicodeWidthChar::width(c).unwrap_or(0);
                if self.pending_word_width + cw > self.max_width {
                    if self.current_width > 0 {
                        self.flush_current_line(output);
                        self.pending_spaces.clear();
                        self.pending_spaces_width = 0;
                    }
                    if self.pending_word_width + cw > self.max_width && self.pending_word_width > 0 {
                        self.current_line.push_str(&self.pending_word);
                        self.apply_pending_ansi();
                        self.flush_current_line(output);
                        self.pending_word.clear();
                        self.pending_word_width = 0;
                    }
                }
                self.pending_word.push(c);
                self.pending_word_width += cw;
            }
        }
        self.commit_pending_word(output);
        if visible_width(&self.current_line) > 0 || output.is_empty() {
            output.push(self.current_line);
        }
    }
}

pub fn wrap_to_width(content: &str, max_width: usize) -> Vec<String> {
    let max_width = max_width.max(1);
    let mut output = Vec::new();
    for line in content.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            output.push(String::new());
            continue;
        }
        LineWrapper::new(line, max_width).wrap(&mut output);
    }
    if output.is_empty() {
        output.push(String::new());
    }
    output
}

pub fn wrap_words_to_width(content: &str, width: usize) -> Vec<String> {
    wrap_plain_text(content, width)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualTruncateResult {
    pub visual_lines: Vec<String>,
    pub skipped_count: usize,
}

pub fn truncate_to_visual_lines(text: &str, max_visual_lines: usize, width: usize) -> VisualTruncateResult {
    if text.is_empty() {
        return VisualTruncateResult {
            visual_lines: Vec::new(),
            skipped_count: 0,
        };
    }
    let all_lines = wrap_plain_text(text, width.max(1));
    if all_lines.len() <= max_visual_lines {
        return VisualTruncateResult {
            visual_lines: all_lines,
            skipped_count: 0,
        };
    }
    let skipped_count = all_lines.len() - max_visual_lines;
    let visual_lines = all_lines[skipped_count..].to_vec();
    VisualTruncateResult {
        visual_lines,
        skipped_count,
    }
}
