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
            .flat_map(|line| wrap_styled_line(line, inner_width))
            .collect();
        self.render_lines(&lines)
    }

    pub fn render_line(&self, content: &str) -> String {
        let mut rendered = self.render_lines(&[content.to_string()]);
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
        format!(
            "{style}{}{content}{style}{}{style:#}\n",
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

fn wrap_styled_line(content: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut active_sgr = String::new();
    let mut current_width = 0;
    let mut offset = 0;

    while offset < content.len() {
        if content.as_bytes()[offset..].starts_with(b"\x1b[")
            && let Some(end) = content[offset..].find('m')
        {
            let end = offset + end + 1;
            let sequence = &content[offset..end];
            current.push_str(sequence);
            if sequence == "\x1b[0m" {
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
        let rendered = BlockFormat::new(background(), 12).render_line("\x1b[31merror\x1b[0m");
        assert_eq!(visible_width(&rendered), 12);
        assert!(rendered.matches("\x1b[40m").count() >= 2);
    }
}
