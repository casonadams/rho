#[cfg(test)]
mod tests;
pub(crate) mod wrap;

pub(crate) use wrap::{ANSI_PATTERN, visible_width};

use anstyle::Style;
use wrap::{wrap_plain_text, wrap_styled_line};

const HORIZONTAL_PADDING: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlockMode {
    #[default]
    Fill,
    Border,
}

pub struct BlockFormat {
    style: Style,
    width: usize,
    vertical_padding: bool,
    mode: BlockMode,
    border_style: Style,
}

impl BlockFormat {
    pub fn new(style: Style, width: usize) -> Self {
        Self {
            style,
            width,
            vertical_padding: false,
            mode: BlockMode::Fill,
            border_style: Style::new(),
        }
    }

    pub fn border(border_style: Style, width: usize) -> Self {
        Self {
            style: Style::new(),
            width,
            vertical_padding: false,
            mode: BlockMode::Border,
            border_style,
        }
    }

    pub fn with_mode(mut self, mode: BlockMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_border_style(mut self, border_style: Style) -> Self {
        self.border_style = border_style;
        self
    }

    pub fn with_vertical_padding(mut self) -> Self {
        self.vertical_padding = true;
        self
    }

    pub fn render_plain(&self, content: &str) -> String {
        let inner_width = self.inner_width();
        let lines = wrap_plain_text(content, inner_width);
        self.render_lines(&lines)
    }

    pub fn render_styled(&self, content: &str) -> String {
        let inner_width = self.inner_width();
        let wrap_style = match self.mode {
            BlockMode::Fill => self.style,
            BlockMode::Border => Style::new(),
        };
        let lines: Vec<String> = content
            .lines()
            .flat_map(|line| wrap_styled_line(line, inner_width, wrap_style))
            .collect();
        self.render_lines(&lines)
    }

    pub fn render_line(&self, content: &str) -> String {
        let inner_width = self.inner_width();
        let wrap_style = match self.mode {
            BlockMode::Fill => self.style,
            BlockMode::Border => Style::new(),
        };
        let lines = wrap_styled_line(content, inner_width, wrap_style);
        let mut rendered = self.render_lines(&lines);
        rendered.pop();
        rendered
    }

    fn inner_width(&self) -> usize {
        match self.mode {
            BlockMode::Fill => self.width.saturating_sub(HORIZONTAL_PADDING * 2).max(1),
            BlockMode::Border => self.width.saturating_sub(4).max(1),
        }
    }

    fn render_lines(&self, lines: &[String]) -> String {
        match self.mode {
            BlockMode::Fill => {
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
            BlockMode::Border => {
                let mut output = String::new();
                output.push_str(&self.top_border_line());
                for line in lines {
                    output.push_str(&self.border_content_line(line));
                }
                output.push_str(&self.bottom_border_line());
                output
            }
        }
    }

    fn top_border_line(&self) -> String {
        let style = self.border_style;
        if self.width < 2 {
            format!("{style}╭{style:#}\n")
        } else {
            let inner = self.width.saturating_sub(2);
            format!("{style}╭{}╮{style:#}\n", "─".repeat(inner))
        }
    }

    fn bottom_border_line(&self) -> String {
        let style = self.border_style;
        if self.width < 2 {
            format!("{style}╰{style:#}\n")
        } else {
            let inner = self.width.saturating_sub(2);
            format!("{style}╰{}╯{style:#}\n", "─".repeat(inner))
        }
    }

    fn border_content_line(&self, content: &str) -> String {
        let style = self.border_style;
        if self.width < 2 {
            return format!("{style}│{style:#}\n");
        }
        if self.width < 4 {
            let inner = self.width.saturating_sub(2);
            let text = if inner == 0 { "" } else { content };
            let visible = visible_width(text);
            let trailing = inner.saturating_sub(visible);
            return format!("{style}│{style:#}{text}{}{style}│{style:#}\n", " ".repeat(trailing));
        }
        let inner_width = self.width.saturating_sub(4);
        let visible = visible_width(content);
        let trailing = inner_width.saturating_sub(visible);
        format!(
            "{style}│{style:#} {content}{}{style} │{style:#}\n",
            " ".repeat(trailing)
        )
    }

    fn padded_line(&self, content: &str) -> String {
        let pad = if self.width >= HORIZONTAL_PADDING * 2 {
            HORIZONTAL_PADDING
        } else {
            0
        };
        let visible = visible_width(content);
        let occupied = pad.saturating_add(visible);
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
            " ".repeat(pad),
            " ".repeat(trailing)
        )
    }
}

pub fn terminal_width() -> usize {
    crossterm::terminal::size()
        .map(|(columns, _)| usize::from(columns.saturating_sub(1).max(1)))
        .unwrap_or(79)
}
