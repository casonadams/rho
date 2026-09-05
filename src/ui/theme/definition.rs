use super::Theme;
use super::color::parse_color;
use anstyle::{Color, Style};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ThemeDef {
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub is_light: bool,

    #[serde(alias = "bg")]
    pub background: Option<String>,
    #[serde(alias = "fg")]
    pub foreground: Option<String>,

    #[serde(alias = "color0")]
    pub black: Option<String>,
    #[serde(alias = "color1")]
    pub red: Option<String>,
    #[serde(alias = "color2")]
    pub green: Option<String>,
    #[serde(alias = "color3")]
    pub yellow: Option<String>,
    #[serde(alias = "color4")]
    pub blue: Option<String>,
    #[serde(alias = "color5")]
    pub magenta: Option<String>,
    #[serde(alias = "color6")]
    pub cyan: Option<String>,
    #[serde(alias = "color7")]
    pub white: Option<String>,

    #[serde(alias = "color8")]
    pub bright_black: Option<String>,
    #[serde(alias = "color9")]
    pub bright_red: Option<String>,
    #[serde(alias = "color10")]
    pub bright_green: Option<String>,
    #[serde(alias = "color11")]
    pub bright_yellow: Option<String>,
    #[serde(alias = "color12")]
    pub bright_blue: Option<String>,
    #[serde(alias = "color13")]
    pub bright_magenta: Option<String>,
    #[serde(alias = "color14")]
    pub bright_cyan: Option<String>,
    #[serde(alias = "color15")]
    pub bright_white: Option<String>,
}

impl ThemeDef {
    pub fn into_theme(&self, name: &str) -> Theme {
        let mut theme = Theme {
            name: name.to_string(),
            is_light: self.is_light,
            background: self.slot(&self.background),
            foreground: self.slot(&self.foreground),
            ..Default::default()
        };

        // Fixed mapping from palette slots to the roles rho needs, mirroring
        // the ANSI colors of the default theme.
        if let Some(c) = self.slot(&self.cyan) {
            theme.prompt = Style::new().fg_color(Some(c));
            theme.highlight = Style::new().fg_color(Some(c));
            theme.code_inline = Style::new().fg_color(Some(c));
            theme.heading_h1 = Style::new().fg_color(Some(c));
        }
        if let Some(c) = self.slot(&self.blue) {
            theme.tool_header = Style::new().fg_color(Some(c));
            theme.heading_h2 = Style::new().fg_color(Some(c));
        }
        if let Some(c) = self.slot(&self.green) {
            theme.tool_ok = Style::new().fg_color(Some(c));
        }
        if let Some(c) = self.slot(&self.red) {
            theme.tool_err = Style::new().fg_color(Some(c));
        }
        if let Some(c) = self.slot(&self.yellow) {
            theme.warning = Style::new().fg_color(Some(c));
        }
        if let Some(c) = self.slot(&self.magenta) {
            theme.skill_tag = Style::new().fg_color(Some(c)).bold();
        }
        if let Some(c) = self.slot(&self.bright_black) {
            theme.thinking = Style::new().fg_color(Some(c));
            theme.heading_h3 = Style::new().fg_color(Some(c));
            theme.dimmed = Style::new().fg_color(Some(c));
        }
        // Blocks sit on the terminal's surface tint (color0), one step away from
        // the region background, so panels stay visible without hiding dimmed text.
        if let Some(c) = self.slot(&self.black) {
            theme.user_message_bg = Style::new().bg_color(Some(c));
            theme.tool_success_bg = Style::new().bg_color(Some(c));
            theme.tool_error_bg = Style::new().bg_color(Some(c));
        }

        theme
    }

    fn slot(&self, value: &Option<String>) -> Option<Color> {
        value.as_deref().and_then(parse_color)
    }
}
