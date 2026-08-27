use anstyle::Style;
use regex::Regex;
use std::sync::LazyLock;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

static ANSI_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*m").expect("valid ANSI escape pattern"));

const HORIZONTAL_PADDING: usize = 2;

pub struct BlockFormat {
    style: Style,
    width: usize,
    vertical_padding: bool,
}

impl BlockFormat {
    pub fn new(style: Style, width: usize) -> Self {
        Self {
            style,
            width,
            vertical_padding: false,
        }
    }

    pub fn with_vertical_padding(mut self) -> Self {
        self.vertical_padding = true;
        self
    }

    pub fn render_plain(&self, content: &str) -> String {
        let inner_width = self.width.saturating_sub(HORIZONTAL_PADDING * 2).max(1);
        let lines = wrap_plain_text(content, inner_width);
        self.render_lines(&lines)
    }

    pub fn render_styled(&self, content: &str) -> String {
        let inner_width = self.width.saturating_sub(HORIZONTAL_PADDING * 2).max(1);
        let lines: Vec<String> = content
            .lines()
            .flat_map(|line| wrap_styled_line(line, inner_width, self.style))
            .collect();
        self.render_lines(&lines)
    }

    pub fn render_line(&self, content: &str) -> String {
        let inner_width = self.width.saturating_sub(HORIZONTAL_PADDING * 2).max(1);
        let lines = wrap_styled_line(content, inner_width, self.style);
        let mut rendered = self.render_lines(&lines);
        rendered.pop();
        rendered
    }

    fn render_lines(&self, lines: &[String]) -> String {
        let mut output = String::new();
        if self.vertical_padding {
            output.push_str(&self.padded_line(""));
        }
        for line in lines {
            output.push_str(&self.padded_line(line));
        }
        if self.vertical_padding {
            output.push_str(&self.padded_line(""));
        }
        output
    }

    fn padded_line(&self, content: &str) -> String {
        let visible = visible_width(content);
        let occupied = HORIZONTAL_PADDING.saturating_add(visible);
        let trailing = self.width.saturating_sub(occupied);
        let style = self.style;
        let bg_str = style.render().to_string();
        let reset_str = if bg_str.is_empty() {
            String::new()
        } else {
            "\x1b[0m".to_string()
        };
        format!(
            "{style}{}{content}{style}{}{reset_str}\n",
            " ".repeat(HORIZONTAL_PADDING.min(self.width)),
            " ".repeat(trailing)
        )
    }
}

pub fn terminal_width() -> usize {
    crossterm::terminal::size()
        .map(|(columns, _)| usize::from(columns.saturating_sub(1).max(1)))
        .unwrap_or(79)
}

fn sgr_resets_background(sequence: &str) -> bool {
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

fn wrap_styled_line(content: &str, width: usize, bg_style: Style) -> Vec<String> {
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

fn wrap_plain_text(content: &str, width: usize) -> Vec<String> {
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

fn visible_width(content: &str) -> usize {
    UnicodeWidthStr::width(ANSI_PATTERN.replace_all(content, "").as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anstyle::{AnsiColor, Color};

    fn background() -> Style {
        Style::new().bg_color(Some(Color::Ansi(AnsiColor::Black)))
    }

    #[test]
    fn plain_blocks_wrap_and_pad_to_the_requested_width() {
        let rendered = BlockFormat::new(background(), 10)
            .with_vertical_padding()
            .render_plain("abcdefghij");
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 4);
        assert!(lines.iter().all(|line| visible_width(line) == 10));
        assert!(rendered.contains("abcdef"));
        assert!(rendered.contains("ghij"));
    }

    #[test]
    fn styled_blocks_wrap_to_full_width_and_preserve_active_color() {
        let rendered = BlockFormat::new(background(), 12).render_styled("\x1b[36mabcdefghijkl\x1b[0m");
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|line| visible_width(line) == 12));
        assert!(lines[1].contains("\x1b[36mijkl"));
    }

    #[test]
    fn styled_content_keeps_its_background_after_an_inner_reset() {
        let rendered = BlockFormat::new(background(), 20).render_line("\x1b[31merror\x1b[0m text");
        assert_eq!(visible_width(&rendered), 20);
        assert!(rendered.contains("\x1b[0m\x1b[40m text"));
        assert!(rendered.ends_with("\x1b[0m"));
    }

    #[test]
    fn multiline_styled_blocks_preserve_background_across_resets_and_blank_lines() {
        let content = "\x1b[1m\x1b[31mbold red\x1b[0m\n\n\x1b[32m+ line 2\x1b[0m extra";
        let rendered = BlockFormat::new(background(), 24)
            .with_vertical_padding()
            .render_styled(content);
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 5);
        for line in &lines {
            assert_eq!(visible_width(line), 24);
            assert!(line.starts_with("\x1b[40m  "));
        }
        assert!(rendered.contains("\x1b[0m\x1b[40m extra"));
    }

    #[test]
    fn compound_and_color_resets_are_detected_correctly() {
        assert!(sgr_resets_background("\x1b[m"));
        assert!(sgr_resets_background("\x1b[0m"));
        assert!(sgr_resets_background("\x1b[49m"));
        assert!(sgr_resets_background("\x1b[0;31m"));
        assert!(sgr_resets_background("\x1b[31;0m"));
        assert!(!sgr_resets_background("\x1b[31m"));
        assert!(!sgr_resets_background("\x1b[1;32m"));
        assert!(!sgr_resets_background("\x1b[38;2;255;0;0m"));
        assert!(!sgr_resets_background("\x1b[38;5;0m"));
    }
}
