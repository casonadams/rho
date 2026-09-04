#[cfg(test)]
mod tests;
pub(crate) mod wrap;

pub(crate) use wrap::{ANSI_PATTERN, visible_width};

use anstyle::Style;
use wrap::{wrap_plain_text, wrap_styled_line};

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
        let inner_width = self.width.max(1);
        let lines = wrap_plain_text(content, inner_width);
        self.render_lines(&lines)
    }

    pub fn render_styled(&self, content: &str) -> String {
        let inner_width = self.width.max(1);
        let lines: Vec<String> = content
            .lines()
            .flat_map(|line| wrap_styled_line(line, inner_width, self.style))
            .collect();
        self.render_lines(&lines)
    }

    pub fn render_line(&self, content: &str) -> String {
        let inner_width = self.width.max(1);
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
        let trailing = self.width.saturating_sub(visible);
        let style = self.style;
        let bg_str = style.render().to_string();
        let reset_str = if bg_str.is_empty() {
            String::new()
        } else {
            "\x1b[0m".to_string()
        };
        format!("{style}{content}{style}{}{reset_str}\n", " ".repeat(trailing))
    }
}

pub fn terminal_width() -> usize {
    crossterm::terminal::size()
        .map(|(columns, _)| usize::from(columns.saturating_sub(1).max(1)))
        .unwrap_or(79)
}
