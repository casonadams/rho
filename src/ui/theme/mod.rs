//! Universal native theme: semantic ANSI-16 colors that adopt the user's
//! terminal palette, plus a terminal-derived dim for secondary text.
//!
//! `Theme::default()` is pure ANSI with no I/O. `detect()` (interactive
//! startup only) queries the terminal's foreground/background via OSC 10/11
//! and computes a block fill blended from those reported colors.

pub mod terminal;

#[cfg(test)]
mod tests;

pub use terminal::{detect, detect_with_config};

use anstyle::{AnsiColor, Color, RgbColor, Style};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlockStyle {
    #[default]
    Solid,
    Border,
}

#[derive(Debug, Clone, PartialEq)]
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
    pub block_style: BlockStyle,
    pub user_border: Style,
    pub agent_border: Style,
    pub tool_border: Style,
    pub bash_success_border: Style,
    pub bash_error_border: Style,
    pub block_agent_output: bool,
}

impl Theme {
    pub fn tool_title_style(&self, is_error: bool) -> Style {
        if is_error {
            self.tool_err.bold()
        } else {
            Style::new().bold()
        }
    }

    pub fn block(&self, border_style: Style, width: usize) -> crate::ui::block::BlockFormat {
        match self.block_style {
            BlockStyle::Solid => crate::ui::block::BlockFormat::new(self.block_fill, width),
            BlockStyle::Border => crate::ui::block::BlockFormat::border(border_style, width),
        }
    }

    pub fn user_block(&self, width: usize) -> crate::ui::block::BlockFormat {
        self.block(self.user_border, width)
    }

    pub fn tool_block(&self, is_bash: bool, is_error: bool, width: usize) -> crate::ui::block::BlockFormat {
        let border = if is_bash {
            if is_error {
                self.bash_error_border
            } else {
                self.bash_success_border
            }
        } else if is_error {
            self.tool_err
        } else {
            self.tool_border
        };
        self.block(border, width)
    }

    pub fn agent_block(&self, width: usize) -> crate::ui::block::BlockFormat {
        self.block(self.agent_border, width)
    }

    pub fn apply_ui_config(&mut self, ui: &rho_harness_core::config::UiConfig) {
        if let Some(ref style) = ui.block_style {
            match style.trim().to_lowercase().as_str() {
                "border" | "outline" => self.block_style = BlockStyle::Border,
                "solid" | "fill" => self.block_style = BlockStyle::Solid,
                _ => {}
            }
        }
        if let Some(ref color) = ui.user_border
            && let Some(c) = parse_color(color)
        {
            self.user_border = Style::new().fg_color(Some(c));
        }
        if let Some(ref color) = ui.agent_border
            && let Some(c) = parse_color(color)
        {
            self.agent_border = Style::new().fg_color(Some(c));
        }
        if let Some(ref color) = ui.tool_border
            && let Some(c) = parse_color(color)
        {
            self.tool_border = Style::new().fg_color(Some(c));
        }
        if let Some(ref color) = ui.bash_success_border
            && let Some(c) = parse_color(color)
        {
            self.bash_success_border = Style::new().fg_color(Some(c));
        }
        if let Some(ref color) = ui.bash_error_border
            && let Some(c) = parse_color(color)
        {
            self.bash_error_border = Style::new().fg_color(Some(c));
        }
        if let Some(val) = ui.agent_block_output {
            self.block_agent_output = val;
        }
    }
}

pub fn parse_color(s: &str) -> Option<Color> {
    let trimmed = s.trim();
    if let Some(hex) = trimmed.strip_prefix('#') {
        return parse_hex_color(hex);
    }
    match trimmed.to_lowercase().as_str() {
        "black" => Some(Color::Ansi(AnsiColor::Black)),
        "red" => Some(Color::Ansi(AnsiColor::Red)),
        "green" => Some(Color::Ansi(AnsiColor::Green)),
        "yellow" => Some(Color::Ansi(AnsiColor::Yellow)),
        "blue" => Some(Color::Ansi(AnsiColor::Blue)),
        "magenta" | "purple" => Some(Color::Ansi(AnsiColor::Magenta)),
        "cyan" => Some(Color::Ansi(AnsiColor::Cyan)),
        "white" => Some(Color::Ansi(AnsiColor::White)),
        "bright_black" | "gray" | "grey" => Some(Color::Ansi(AnsiColor::BrightBlack)),
        "bright_red" => Some(Color::Ansi(AnsiColor::BrightRed)),
        "bright_green" => Some(Color::Ansi(AnsiColor::BrightGreen)),
        "bright_yellow" => Some(Color::Ansi(AnsiColor::BrightYellow)),
        "bright_blue" => Some(Color::Ansi(AnsiColor::BrightBlue)),
        "bright_magenta" => Some(Color::Ansi(AnsiColor::BrightMagenta)),
        "bright_cyan" => Some(Color::Ansi(AnsiColor::BrightCyan)),
        "bright_white" => Some(Color::Ansi(AnsiColor::BrightWhite)),
        _ => {
            if trimmed.len() == 6 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
                parse_hex_color(trimmed)
            } else {
                None
            }
        }
    }
}

fn parse_hex_color(hex: &str) -> Option<Color> {
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(RgbColor(r, g, b)))
}

fn foreground(color: AnsiColor) -> Style {
    Style::new().fg_color(Some(Color::Ansi(color)))
}

impl Default for Theme {
    fn default() -> Self {
        let grey = Style::new().fg_color(Some(Color::Ansi(AnsiColor::BrightBlack)));
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
            block_style: BlockStyle::Solid,
            user_border: grey,
            agent_border: grey,
            tool_border: grey,
            bash_success_border: grey,
            bash_error_border: foreground(AnsiColor::Red),
            block_agent_output: false,
        }
    }
}
