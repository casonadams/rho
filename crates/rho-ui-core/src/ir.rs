use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentBlock {
    Paragraph(Vec<InlineSpan>),
    Heading {
        level: u8,
        content: Vec<InlineSpan>,
    },
    CodeFence {
        language: String,
        content: String,
        lines: Vec<HighlightedLine>,
    },
    Diff {
        file_path: Option<String>,
        old_line: Option<usize>,
        new_line: Option<usize>,
        hunks: Vec<DiffHunk>,
    },
    Table {
        headers: Vec<Vec<InlineSpan>>,
        rows: Vec<Vec<Vec<InlineSpan>>>,
    },
    Diagram {
        kind: DiagramKind,
        content: String,
    },
    Thinking {
        content: String,
        is_complete: bool,
        duration_ms: Option<u64>,
    },
    ToolCall(ToolInvocation),
    ToolResult {
        invocation: ToolInvocation,
        output: String,
        is_error: bool,
        duration_ms: Option<u64>,
        images: Vec<ImageAttachment>,
    },
    Notice {
        text: String,
        is_error: bool,
    },
    UserPrompt {
        text: String,
        images: Vec<ImageAttachment>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DiagramKind {
    #[default]
    Mermaid,
    Ascii,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InlineSpan {
    Text(String),
    Bold(String),
    Italic(String),
    Code(String),
    Strikethrough(String),
    Link { label: String, url: String },
    Styled { text: String, style: StyleToken },
}

impl InlineSpan {
    pub fn plain_text(&self) -> &str {
        match self {
            Self::Text(s)
            | Self::Bold(s)
            | Self::Italic(s)
            | Self::Code(s)
            | Self::Strikethrough(s)
            | Self::Styled { text: s, .. } => s.as_str(),
            Self::Link { label, .. } => label.as_str(),
        }
    }

    pub fn to_ratatui_span(&self, tokens: &ThemeTokens) -> ratatui::text::Span<'static> {
        match self {
            Self::Text(s) => ratatui::text::Span::raw(s.clone()),
            Self::Bold(s) => ratatui::text::Span::styled(s.clone(), Style::default().add_modifier(Modifier::BOLD)),
            Self::Italic(s) => ratatui::text::Span::styled(s.clone(), Style::default().add_modifier(Modifier::ITALIC)),
            Self::Code(s) => {
                ratatui::text::Span::styled(s.clone(), Style::default().fg(tokens.accent).bg(tokens.code_bg))
            }
            Self::Strikethrough(s) => {
                ratatui::text::Span::styled(s.clone(), Style::default().add_modifier(Modifier::CROSSED_OUT))
            }
            Self::Link { label, .. } => ratatui::text::Span::styled(
                label.clone(),
                Style::default().fg(tokens.link).add_modifier(Modifier::UNDERLINED),
            ),
            Self::Styled { text, style } => ratatui::text::Span::styled(text.clone(), tokens.resolve_style(style)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HighlightedLine {
    pub spans: Vec<InlineSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeType {
    Added,
    Removed,
    Equal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffHunk {
    pub change: ChangeType,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub line: String,
    pub inline_spans: Vec<InlineSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StyleToken {
    Primary,
    Secondary,
    Accent,
    Success,
    Warning,
    Error,
    Dimmed,
    Bold,
    Italic,
    Underline,
    Custom {
        fg: Option<Color>,
        bg: Option<Color>,
        bold: bool,
        dim: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageAttachment {
    pub data_base64: String,
    pub mime_type: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInvocation {
    pub id: String,
    pub name: String,
    pub args_summary: String,
    pub raw_args: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeTokens {
    pub primary: Color,
    pub secondary: Color,
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub dimmed: Color,
    pub background: Color,
    pub block_fill: Color,
    pub code_bg: Color,
    pub link: Color,
    pub border: Color,
}

impl Default for ThemeTokens {
    fn default() -> Self {
        Self {
            primary: Color::Reset,
            secondary: Color::DarkGray,
            accent: Color::Cyan,
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            dimmed: Color::DarkGray,
            background: Color::Reset,
            block_fill: Color::Rgb(24, 24, 27),
            code_bg: Color::Rgb(39, 39, 42),
            link: Color::Blue,
            border: Color::DarkGray,
        }
    }
}

impl ThemeTokens {
    pub fn resolve_style(&self, token: &StyleToken) -> Style {
        match token {
            StyleToken::Primary => Style::default().fg(self.primary),
            StyleToken::Secondary => Style::default().fg(self.secondary),
            StyleToken::Accent => Style::default().fg(self.accent),
            StyleToken::Success => Style::default().fg(self.success),
            StyleToken::Warning => Style::default().fg(self.warning),
            StyleToken::Error => Style::default().fg(self.error),
            StyleToken::Dimmed => Style::default().fg(self.dimmed).add_modifier(Modifier::DIM),
            StyleToken::Bold => Style::default().add_modifier(Modifier::BOLD),
            StyleToken::Italic => Style::default().add_modifier(Modifier::ITALIC),
            StyleToken::Underline => Style::default().add_modifier(Modifier::UNDERLINED),
            StyleToken::Custom { fg, bg, bold, dim } => {
                let mut style = Style::default();
                if let Some(c) = fg {
                    style = style.fg(*c);
                }
                if let Some(c) = bg {
                    style = style.bg(*c);
                }
                if *bold {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if *dim {
                    style = style.add_modifier(Modifier::DIM);
                }
                style
            }
        }
    }
}
