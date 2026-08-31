use crate::ui::block::BlockFormat;
use crate::ui::render::{
    format_duration_ms, format_edit_diff, format_thinking_block, format_tool_args_summary, format_write_preview,
    read_summary_parts, tool_title_style, webfetch_content_kind,
};
use crate::ui::theme::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WelcomeItem {
    pub version: String,
    pub model: String,
    pub provider: String,
    pub auto_approve: bool,
    pub resumed: bool,
    pub location: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolItem {
    pub name: String,
    pub arguments: serde_json::Value,
    pub is_error: bool,
    pub output: String,
    pub output_summary: String,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptItem {
    Welcome(WelcomeItem),
    UserMessage(String),
    AssistantText(String),
    Thinking(String),
    Tool(ToolItem),
    Notice(String),
}

#[derive(Debug, Clone, Copy)]
pub struct TranscriptRenderInput<'a> {
    pub item: &'a TranscriptItem,
    pub theme: &'a Theme,
    pub width: usize,
    pub tools_expanded: bool,
}

pub fn render_transcript_item(input: TranscriptRenderInput<'_>) -> String {
    let width = input.width.max(20);
    let theme = input.theme;
    match input.item {
        TranscriptItem::Welcome(welcome) => {
            let highlight = theme.highlight;
            let dim = theme.dimmed;
            let session = if welcome.resumed {
                "resumed session"
            } else {
                "new session"
            };
            let approval = if welcome.auto_approve {
                "auto-approve"
            } else {
                "confirm changes"
            };
            format!(
                "\n{highlight}rho{highlight:#} {dim}v{}{dim:#}\n{} {dim}via {} | {session}{dim:#}\n{dim}{} | {approval}{dim:#}\n{dim}/help commands | Tab complete | Ctrl+C cancel | Ctrl+D exit{dim:#}\n\n",
                welcome.version, welcome.model, welcome.provider, welcome.location
            )
        }
        TranscriptItem::UserMessage(text) => {
            let block = BlockFormat::new(theme.user_message_bg, width)
                .with_vertical_padding()
                .render_plain(text);
            format!("\n{block}")
        }
        TranscriptItem::AssistantText(text) => {
            let mut md = crate::ui::markdown::MarkdownRenderer::default();
            let rendered = md.render_token(text, theme);
            let flushed = md.flush(theme);
            format!("{rendered}{flushed}")
        }
        TranscriptItem::Thinking(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                String::new()
            } else {
                format_thinking_block(trimmed, theme)
            }
        }
        TranscriptItem::Tool(tool) => {
            let background = if tool.is_error {
                theme.tool_error_bg
            } else {
                theme.tool_success_bg
            };
            let title = tool_title_style(tool.is_error);
            let accent = theme.highlight;
            let summary = format_tool_args_summary(&tool.name, &tool.arguments);

            let mut content = if tool.name == "read" && !tool.is_error {
                let (path, range) = read_summary_parts(&tool.arguments);
                let range_style = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Yellow.into()));
                format!(
                    "{title}read{title:#} {accent}{path}{accent:#}{}",
                    range.map_or_else(String::new, |range| format!("{range_style}{range}{range_style:#}"))
                )
            } else if tool.name == "webfetch" && !tool.is_error {
                let url = tool
                    .arguments
                    .get("url")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let status = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Yellow.into()));
                let dim = theme.dimmed;
                let kind = webfetch_content_kind(&tool.arguments);
                format!(
                    "{title}webfetch{title:#}\n{accent}{url}{accent:#}\n{status}fetched ({kind}){status:#}\n{dim}{url}{dim:#}"
                )
            } else {
                format!("{title}{}{title:#} {accent}{summary}{accent:#}", tool.name)
            };

            if !tool.is_error && tool.name == "edit" {
                if let Some(diff) = format_edit_diff(&tool.arguments, theme) {
                    content.push('\n');
                    content.push_str(&diff);
                }
            } else if !tool.is_error && tool.name == "write" {
                if let Some(preview) = format_write_preview(&tool.arguments, theme) {
                    content.push('\n');
                    content.push_str(&preview);
                }
            } else if tool.name == "bash" || tool.is_error {
                content.push_str("\n\n");
                if !tool.output.is_empty() {
                    content.push_str(&tool.output);
                } else if !tool.output_summary.is_empty() {
                    content.push_str(&tool.output_summary);
                }
            }

            if let Some(duration_ms) = tool.duration_ms {
                let dim = theme.dimmed;
                content.push('\n');
                content.push_str(&format!("{dim}Took {}{dim:#}", format_duration_ms(duration_ms)));
            }

            let block = BlockFormat::new(background, width)
                .with_vertical_padding()
                .render_styled(&content);
            format!("\n{block}")
        }
        TranscriptItem::Notice(text) => text.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_transcript_user_message() {
        let theme = Theme::default();
        let item = TranscriptItem::UserMessage("hello world".into());
        let rendered = render_transcript_item(TranscriptRenderInput {
            item: &item,
            theme: &theme,
            width: 60,
            tools_expanded: false,
        });
        assert!(rendered.contains("hello world"));
    }

    #[test]
    fn render_transcript_tool_preserves_full_output() {
        let theme = Theme::default();
        let item = TranscriptItem::Tool(ToolItem {
            name: "bash".into(),
            arguments: serde_json::json!({"command": "cargo test"}),
            is_error: false,
            output: "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\nline10".into(),
            output_summary: "summary".into(),
            duration_ms: Some(150),
        });

        let rendered = render_transcript_item(TranscriptRenderInput {
            item: &item,
            theme: &theme,
            width: 80,
            tools_expanded: false,
        });
        assert!(rendered.contains("line1"));
        assert!(rendered.contains("line10"));
        assert!(rendered.contains("Took 150ms"));
    }
}
