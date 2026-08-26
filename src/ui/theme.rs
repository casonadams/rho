use anstyle::{AnsiColor, Color, Effects, Style};

#[derive(Clone)]
pub struct Theme {
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
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            prompt: Style::new().fg_color(Some(Color::Ansi(AnsiColor::Cyan))),
            assistant: Style::new(),
            thinking: Style::new().effects(Effects::DIMMED),
            tool_header: Style::new().fg_color(Some(Color::Ansi(AnsiColor::Blue))),
            tool_ok: Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green))),
            tool_err: Style::new().fg_color(Some(Color::Ansi(AnsiColor::Red))),
            highlight: Style::new().fg_color(Some(Color::Ansi(AnsiColor::Cyan))),
            code_inline: Style::new().fg_color(Some(Color::Ansi(AnsiColor::Magenta))),
            heading_h1: Style::new().fg_color(Some(Color::Ansi(AnsiColor::Cyan))),
            heading_h2: Style::new().fg_color(Some(Color::Ansi(AnsiColor::Blue))),
            heading_h3: Style::new().effects(Effects::DIMMED),
            dimmed: Style::new().effects(Effects::DIMMED),
        }
    }
}
