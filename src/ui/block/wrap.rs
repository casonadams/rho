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
}

impl WrapState<'_> {
    fn push_char(&mut self, character: char, character_width: usize) {
        if self.current_width > 0 && self.current_width + character_width > self.width {
            self.lines.push(std::mem::take(&mut self.current));
            self.current.push_str(&self.active_sgr);
            self.current_width = 0;
        }
        self.current.push(character);
        self.current_width += character_width;
    }

    fn consume_sgr(&mut self, content: &str) {
        let Some(rel) = content[self.offset..].find('m') else {
            return;
        };
        let end = self.offset + rel + 1;
        let sequence = &content[self.offset..end];
        self.current.push_str(sequence);
        if sgr_resets_background(sequence) {
            if !self.bg_code.is_empty() {
                self.current.push_str(&self.bg_code);
            }
            self.active_sgr.clear();
        } else {
            self.active_sgr.push_str(sequence);
        }
        self.offset = end;
    }
}

pub(crate) fn wrap_styled_line(content: &str, width: usize, bg_style: Style) -> Vec<String> {
    let mut lines = Vec::new();
    let mut state = WrapState {
        lines: &mut lines,
        width,
        bg_code: bg_style.render().to_string(),
        current: String::new(),
        active_sgr: String::new(),
        current_width: 0,
        offset: 0,
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

    state.lines.push(state.current);
    lines
}

pub(crate) fn wrap_plain_text(content: &str, width: usize) -> Vec<String> {
    let mut output = Vec::new();
    for line in content.split('\n') {
        let mut current = String::new();
        let mut current_width = 0;
        for character in line.chars() {
            let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
            if current_width > 0 && current_width + character_width > width {
                output.push(std::mem::take(&mut current));
                current_width = 0;
            }
            current.push(character);
            current_width += character_width;
        }
        output.push(current);
    }
    output
}
