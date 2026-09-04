use super::Theme;
use super::color::parse_color;
use anstyle::{Effects, Style};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ThemeDef {
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub is_light: bool,
    pub prompt: Option<String>,
    pub assistant: Option<String>,
    pub thinking: Option<String>,
    pub tool_header: Option<String>,
    pub tool_ok: Option<String>,
    pub tool_err: Option<String>,
    pub highlight: Option<String>,
    pub code_inline: Option<String>,
    pub heading_h1: Option<String>,
    pub heading_h2: Option<String>,
    pub heading_h3: Option<String>,
    pub dimmed: Option<String>,
    pub user_message_bg: Option<String>,
    pub tool_success_bg: Option<String>,
    pub tool_error_bg: Option<String>,
}

impl ThemeDef {
    pub fn into_theme(&self, name: &str) -> Theme {
        let mut theme = Theme {
            name: name.to_string(),
            ..Default::default()
        };

        if let Some(c) = self.prompt.as_deref().and_then(parse_color) {
            theme.prompt = Style::new().fg_color(Some(c));
        }
        if let Some(c) = self.assistant.as_deref().and_then(parse_color) {
            theme.assistant = Style::new().fg_color(Some(c));
        }
        if let Some(c) = self.thinking.as_deref().and_then(parse_color) {
            theme.thinking = Style::new().fg_color(Some(c)).effects(Effects::DIMMED);
        }
        if let Some(c) = self.tool_header.as_deref().and_then(parse_color) {
            theme.tool_header = Style::new().fg_color(Some(c));
        }
        if let Some(c) = self.tool_ok.as_deref().and_then(parse_color) {
            theme.tool_ok = Style::new().fg_color(Some(c));
        }
        if let Some(c) = self.tool_err.as_deref().and_then(parse_color) {
            theme.tool_err = Style::new().fg_color(Some(c));
        }
        if let Some(c) = self.highlight.as_deref().and_then(parse_color) {
            theme.highlight = Style::new().fg_color(Some(c));
        }
        if let Some(c) = self.code_inline.as_deref().and_then(parse_color) {
            theme.code_inline = Style::new().fg_color(Some(c));
        }
        if let Some(c) = self.heading_h1.as_deref().and_then(parse_color) {
            theme.heading_h1 = Style::new().fg_color(Some(c)).bold();
        }
        if let Some(c) = self.heading_h2.as_deref().and_then(parse_color) {
            theme.heading_h2 = Style::new().fg_color(Some(c)).bold();
        }
        if let Some(c) = self.heading_h3.as_deref().and_then(parse_color) {
            theme.heading_h3 = Style::new().fg_color(Some(c)).effects(Effects::DIMMED);
        }
        if let Some(c) = self.dimmed.as_deref().and_then(parse_color) {
            theme.dimmed = Style::new().fg_color(Some(c)).effects(Effects::DIMMED);
        }
        if let Some(c) = self.user_message_bg.as_deref().and_then(parse_color) {
            theme.user_message_bg = Style::new().bg_color(Some(c));
        }
        if let Some(c) = self.tool_success_bg.as_deref().and_then(parse_color) {
            theme.tool_success_bg = Style::new().bg_color(Some(c));
        }
        if let Some(c) = self.tool_error_bg.as_deref().and_then(parse_color) {
            theme.tool_error_bg = Style::new().bg_color(Some(c));
        }

        theme
    }
}
