use super::Theme;
use super::color::parse_color;
use anstyle::Style;
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
    pub warning: Option<String>,
    pub skill_tag: Option<String>,
    pub user_message_bg: Option<String>,
    pub tool_success_bg: Option<String>,
    pub tool_error_bg: Option<String>,
}

impl ThemeDef {
    pub fn into_theme(&self, name: &str) -> Theme {
        let mut theme = Theme {
            name: name.to_string(),
            is_light: self.is_light,
            terminal_bg: self.background.clone(),
            terminal_fg: self.foreground.clone(),
            ..Default::default()
        };

        let c_black = self
            .black
            .as_deref()
            .or(self.background.as_deref())
            .and_then(parse_color);
        let c_red = self.red.as_deref().and_then(parse_color);
        let c_green = self.green.as_deref().and_then(parse_color);
        let c_yellow = self.yellow.as_deref().and_then(parse_color);
        let c_blue = self.blue.as_deref().and_then(parse_color);
        let c_magenta = self.magenta.as_deref().and_then(parse_color);
        let c_cyan = self.cyan.as_deref().and_then(parse_color);
        let c_white = self
            .white
            .as_deref()
            .or(self.foreground.as_deref())
            .and_then(parse_color);

        let c_bright_black = self.bright_black.as_deref().and_then(parse_color);
        let c_bright_magenta = self.bright_magenta.as_deref().and_then(parse_color).or(c_magenta);
        let c_bright_cyan = self.bright_cyan.as_deref().and_then(parse_color).or(c_cyan);

        let prompt_color = self.prompt.as_deref().and_then(parse_color).or(c_cyan);
        if let Some(c) = prompt_color {
            theme.prompt = Style::new().fg_color(Some(c));
        }

        let assistant_color = self.assistant.as_deref().and_then(parse_color).or(c_white);
        if let Some(c) = assistant_color {
            theme.assistant = Style::new().fg_color(Some(c));
        }

        let thinking_color = self.thinking.as_deref().and_then(parse_color).or(c_bright_black);
        if let Some(c) = thinking_color {
            theme.thinking = Style::new().fg_color(Some(c)).dimmed();
        }

        let header_color = self.tool_header.as_deref().and_then(parse_color).or(c_blue);
        if let Some(c) = header_color {
            theme.tool_header = Style::new().fg_color(Some(c));
        }

        let ok_color = self.tool_ok.as_deref().and_then(parse_color).or(c_green);
        if let Some(c) = ok_color {
            theme.tool_ok = Style::new().fg_color(Some(c));
        }

        let err_color = self.tool_err.as_deref().and_then(parse_color).or(c_red);
        if let Some(c) = err_color {
            theme.tool_err = Style::new().fg_color(Some(c));
        }

        let highlight_color = self.highlight.as_deref().and_then(parse_color).or(c_bright_cyan);
        if let Some(c) = highlight_color {
            theme.highlight = Style::new().fg_color(Some(c));
        }

        let code_color = self.code_inline.as_deref().and_then(parse_color).or(c_yellow);
        if let Some(c) = code_color {
            theme.code_inline = Style::new().fg_color(Some(c));
        }

        let h1_color = self
            .heading_h1
            .as_deref()
            .and_then(parse_color)
            .or(c_magenta)
            .or(c_cyan);
        if let Some(c) = h1_color {
            theme.heading_h1 = Style::new().fg_color(Some(c)).bold();
        }

        let h2_color = self.heading_h2.as_deref().and_then(parse_color).or(c_blue);
        if let Some(c) = h2_color {
            theme.heading_h2 = Style::new().fg_color(Some(c)).bold();
        }

        let h3_color = self.heading_h3.as_deref().and_then(parse_color).or(c_cyan);
        if let Some(c) = h3_color {
            theme.heading_h3 = Style::new().fg_color(Some(c)).dimmed();
        }

        let dimmed_color = self.dimmed.as_deref().and_then(parse_color).or(c_bright_black);
        if let Some(c) = dimmed_color {
            theme.dimmed = Style::new().fg_color(Some(c)).dimmed();
        }

        let warning_color = self.warning.as_deref().and_then(parse_color).or(c_yellow);
        if let Some(c) = warning_color {
            theme.warning = Style::new().fg_color(Some(c));
        }

        let skill_color = self.skill_tag.as_deref().and_then(parse_color).or(c_bright_magenta);
        if let Some(c) = skill_color {
            theme.skill_tag = Style::new().fg_color(Some(c)).bold();
        }

        let user_bg = self.user_message_bg.as_deref().and_then(parse_color).or(c_black);
        if let Some(c) = user_bg {
            theme.user_message_bg = Style::new().bg_color(Some(c));
        }

        let tool_ok_bg = self.tool_success_bg.as_deref().and_then(parse_color).or(c_black);
        if let Some(c) = tool_ok_bg {
            theme.tool_success_bg = Style::new().bg_color(Some(c));
        }

        let tool_err_bg = self.tool_error_bg.as_deref().and_then(parse_color).or(c_black);
        if let Some(c) = tool_err_bg {
            theme.tool_error_bg = Style::new().bg_color(Some(c));
        }

        theme
    }
}
