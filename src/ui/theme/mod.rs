//! Universal native theme: semantic ANSI-16 colors that adopt the user's
//! terminal palette, plus a terminal-derived dim for secondary text.
//!
//! `Theme::default()` is pure ANSI with no I/O. `detect()` (interactive
//! startup only) queries the terminal's foreground/background via OSC 10/11
//! and computes a block fill blended from those reported colors.

pub mod terminal;

#[cfg(test)]
mod tests;

pub use terminal::detect;

use anstyle::{AnsiColor, Color, Style};

#[derive(Debug, Clone)]
pub struct Theme {
    /// Detected terminal light/dark mode; selects syntect syntax colors.
    pub is_light: bool,
    pub prompt: Style,
    pub thinking: Style,
    pub tool_header: Style,
    pub tool_ok: Style,
    pub tool_err: Style,
    pub highlight: Style,
    pub code_inline: Style,
    pub heading_h1: Style,
    pub heading_h2: Style,
    pub heading_h3: Style,
    pub dimmed: Style,
    pub warning: Style,
    pub skill_tag: Style,
    /// Full-width container fill behind tool cards, user messages, and notices.
    pub block_fill: Style,
}

impl Theme {
    pub fn tool_title_style(&self, is_error: bool) -> Style {
        if is_error {
            self.tool_err.bold()
        } else {
            Style::new().bold()
        }
    }
}

fn foreground(color: AnsiColor) -> Style {
    Style::new().fg_color(Some(Color::Ansi(color)))
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            is_light: false,
            prompt: foreground(AnsiColor::Cyan),
            thinking: Style::new().dimmed(),
            tool_header: foreground(AnsiColor::Blue),
            tool_ok: foreground(AnsiColor::Green),
            tool_err: foreground(AnsiColor::Red),
            highlight: foreground(AnsiColor::Cyan),
            code_inline: foreground(AnsiColor::Cyan),
            heading_h1: foreground(AnsiColor::Cyan),
            heading_h2: foreground(AnsiColor::Blue),
            heading_h3: Style::new().dimmed(),
            dimmed: Style::new().dimmed(),
            warning: foreground(AnsiColor::Yellow),
            skill_tag: foreground(AnsiColor::Magenta).bold(),
            block_fill: Style::new().bg_color(Some(Color::Ansi(AnsiColor::Black))),
        }
    }
}
