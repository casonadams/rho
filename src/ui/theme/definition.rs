use anstyle::{Color, Style};
use serde::Deserialize;

use super::Theme;
use super::color::parse_color;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ThemeDef {
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub is_light: bool,
    pub background: Option<String>,
    pub foreground: Option<String>,
    #[serde(alias = "color00")]
    pub black: Option<String>,
    #[serde(alias = "color01")]
    pub red: Option<String>,
    #[serde(alias = "color02")]
    pub green: Option<String>,
    #[serde(alias = "color03")]
    pub yellow: Option<String>,
    #[serde(alias = "color04")]
    pub blue: Option<String>,
    #[serde(alias = "color05")]
    pub magenta: Option<String>,
    #[serde(alias = "color06")]
    pub cyan: Option<String>,
    #[serde(alias = "color07")]
    pub white: Option<String>,
    #[serde(alias = "color08")]
    pub bright_black: Option<String>,
    #[serde(alias = "color09")]
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
    fn apply_primary_styles(&self, theme: &mut Theme) {
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
    }

    fn apply_secondary_styles(&self, theme: &mut Theme) {
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
        if let Some(c) = self.slot(&self.black) {
            theme.user_message_bg = Style::new().bg_color(Some(c));
            theme.tool_success_bg = Style::new().bg_color(Some(c));
            theme.tool_error_bg = Style::new().bg_color(Some(c));
        }
    }

    pub fn into_theme(&self, name: &str) -> Theme {
        let mut theme = Theme {
            name: name.to_string(),
            is_light: self.is_light,
            background: self.slot(&self.background),
            foreground: self.slot(&self.foreground),
            ..Default::default()
        };
        self.apply_primary_styles(&mut theme);
        self.apply_secondary_styles(&mut theme);
        theme
    }

    fn slot(&self, value: &Option<String>) -> Option<Color> {
        value.as_deref().and_then(parse_color)
    }
}
