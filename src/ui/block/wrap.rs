use anstyle::Style;
use regex::Regex;
use std::sync::LazyLock;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub(crate) static ANSI_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*m").expect("valid ANSI escape pattern"));

pub(crate) fn visible_width(content: &str) -> usize {
    UnicodeWidthStr::width(ANSI_PATTERN.replace_all(content, "").as_ref())
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
    }
    false
}

pub(crate) fn wrap_styled_line(content: &str, width: usize, bg_style: Style) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut active_sgr = String::new();
    let mut current_width = 0;
    let mut offset = 0;
    let bg_code = bg_style.render().to_string();

    while offset < content.len() {
        if content.as_bytes()[offset..].starts_with(b"\x1b[")
            && let Some(end) = content[offset..].find('m')
        {
            let end = offset + end + 1;
            let sequence = &content[offset..end];
            current.push_str(sequence);
            if sgr_resets_background(sequence) {
                if !bg_code.is_empty() {
                    current.push_str(&bg_code);
                }
                active_sgr.clear();
            } else {
                active_sgr.push_str(sequence);
            }
            offset = end;
            continue;
        }

        let Some(character) = content[offset..].chars().next() else {
            break;
        };
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if current_width > 0 && current_width + character_width > width {
            lines.push(std::mem::take(&mut current));
            current.push_str(&active_sgr);
            current_width = 0;
        }
        current.push(character);
        current_width += character_width;
        offset += character.len_utf8();
    }

    lines.push(current);
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
