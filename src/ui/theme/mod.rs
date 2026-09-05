pub mod builtin;
pub mod color;
pub mod definition;
pub mod registry;

#[cfg(test)]
mod tests;

pub use builtin::{BuiltinTheme, builtin_themes};
pub use definition::ThemeDef;
pub use registry::{ThemeMetadata, ThemeRegistry};

use anstyle::{AnsiColor, Color, Style};

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub is_light: bool,
    pub background: Option<Color>,
    pub foreground: Option<Color>,
    pub prompt: Style,
    pub assistant: Style,
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
    pub user_message_bg: Style,
    pub tool_success_bg: Style,
    pub tool_error_bg: Style,
}

impl Theme {
    pub fn is_ansi(&self) -> bool {
        self.background.is_none() && self.foreground.is_none()
    }

    pub fn is_light(&self) -> bool {
        self.is_light
    }

    pub fn tool_title_style(&self, is_error: bool) -> Style {
        if is_error {
            self.tool_err.bold()
        } else {
            Style::new().bold()
        }
    }
}

fn background(color: AnsiColor) -> Style {
    Style::new().bg_color(Some(Color::Ansi(color)))
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            is_light: false,
            background: None,
            foreground: None,
            prompt: Style::new().fg_color(Some(Color::Ansi(AnsiColor::Cyan))),
            assistant: Style::new(),
            thinking: Style::new().fg_color(Some(Color::Ansi(AnsiColor::BrightBlack))),
            tool_header: Style::new().fg_color(Some(Color::Ansi(AnsiColor::Blue))),
            tool_ok: Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green))),
            tool_err: Style::new().fg_color(Some(Color::Ansi(AnsiColor::Red))),
            highlight: Style::new().fg_color(Some(Color::Ansi(AnsiColor::Cyan))),
            code_inline: Style::new().fg_color(Some(Color::Ansi(AnsiColor::Cyan))),
            heading_h1: Style::new().fg_color(Some(Color::Ansi(AnsiColor::Cyan))),
            heading_h2: Style::new().fg_color(Some(Color::Ansi(AnsiColor::Blue))),
            heading_h3: Style::new().fg_color(Some(Color::Ansi(AnsiColor::BrightBlack))),
            dimmed: Style::new().fg_color(Some(Color::Ansi(AnsiColor::BrightBlack))),
            warning: Style::new().fg_color(Some(Color::Ansi(AnsiColor::Yellow))),
            skill_tag: Style::new().fg_color(Some(Color::Ansi(AnsiColor::Magenta))).bold(),
            user_message_bg: background(AnsiColor::Black),
            tool_success_bg: background(AnsiColor::Black),
            tool_error_bg: background(AnsiColor::Black),
        }
    }
}
