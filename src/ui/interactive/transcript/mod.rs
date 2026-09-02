use crate::ui::block::BlockFormat;
use crate::ui::render::{
    format_duration_ms, format_edit_diff, format_thinking_block, format_tool_args_summary, format_write_preview,
    read_summary_parts, tool_title_style, webfetch_content_kind,
};
use crate::ui::theme::Theme;

#[cfg(test)]
mod tests;

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
            let display_name = match tool.name.as_str() {
                "web_search" | "websearch" => "search",
                "web_fetch" | "webfetch" => "fetch",
                other => other,
            };
            let summary = format_tool_args_summary(&tool.name, &tool.arguments);

            let mut content = if tool.name == "read" && !tool.is_error {
                let (path, range) = read_summary_parts(&tool.arguments);
                let range_style = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Yellow.into()));
                format!(
                    "{title}read{title:#} {accent}{path}{accent:#}{}",
                    range.map_or_else(String::new, |range| format!("{range_style}{range}{range_style:#}"))
                )
            } else if display_name == "fetch" && !tool.is_error {
                let url = tool
                    .arguments
                    .get("url")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let status = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Yellow.into()));
                let kind = webfetch_content_kind(&tool.arguments);
                format!("{title}{display_name}{title:#} {accent}{url}{accent:#}\n{status}fetched ({kind}){status:#}")
            } else {
                format!("{title}{display_name}{title:#} {accent}{summary}{accent:#}")
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
                let raw_output = if !tool.output.is_empty() {
                    &tool.output
                } else {
                    &tool.output_summary
                };
                let clean = raw_output.trim_end();
                if !clean.is_empty() {
                    content.push_str("\n\n");
                    if input.tools_expanded {
                        content.push_str(clean);
                    } else {
                        let truncated =
                            super::layout::truncate_to_visual_lines(clean, 5, width.saturating_sub(4).max(1));
                        if truncated.skipped_count > 0 {
                            let dim = theme.dimmed;
                            content.push_str(&format!(
                                "{dim}... ({} earlier lines, Ctrl+O to expand){dim:#}\n",
                                truncated.skipped_count
                            ));
                        }
                        content.push_str(&truncated.visual_lines.join("\n"));
                    }
                }
            }

            if tool.name == "bash"
                && let Some(duration_ms) = tool.duration_ms
            {
                let dim = theme.dimmed;
                content.push_str("\n\n");
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
