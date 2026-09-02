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
    pub tools: Vec<String>,
    pub skills: Vec<String>,
    pub plugins: Vec<String>,
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

pub fn format_welcome_content(welcome: &WelcomeItem, theme: &Theme) -> String {
    let highlight = theme.highlight;
    let dim = theme.dimmed;

    let mut out = format!(
        "\n{highlight}rho{highlight:#} {dim}v{}{dim:#}\n{dim}Type /help for commands, Tab to complete, Ctrl+C to cancel{dim:#}\n\n",
        welcome.version
    );

    let indent = "  ";
    let wrap_width = 76;

    if !welcome.skills.is_empty() {
        let text = welcome.skills.join(", ");
        let wrapped = crate::ui::interactive::layout::wrap_to_width(&text, wrap_width);
        out.push_str(&format!("{dim}[skills]{dim:#}\n"));
        for line in wrapped {
            out.push_str(&format!("{indent}{line}\n"));
        }
        out.push('\n');
    }

    let mut builtins = Vec::new();
    let mut mcp_groups: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut custom_tools = Vec::new();

    for tool in &welcome.tools {
        match tool.as_str() {
            "read" | "write" | "edit" | "bash" => {
                if !builtins.contains(&tool.as_str()) {
                    builtins.push(tool.as_str());
                }
            }
            "search" | "websearch" | "web_search" => {
                if !builtins.contains(&"search") {
                    builtins.push("search");
                }
            }
            "fetch" | "webfetch" | "web_fetch" => {
                if !builtins.contains(&"fetch") {
                    builtins.push("fetch");
                }
            }
            other => {
                if let Some((server, _)) = other.split_once('_') {
                    *mcp_groups.entry(server.to_string()).or_default() += 1;
                } else if !custom_tools.contains(&other.to_string()) {
                    custom_tools.push(other.to_string());
                }
            }
        }
    }

    if !builtins.is_empty() || !custom_tools.is_empty() {
        let mut all_tools = builtins.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        all_tools.extend(custom_tools);
        let text = all_tools.join(", ");
        let wrapped = crate::ui::interactive::layout::wrap_to_width(&text, wrap_width);
        out.push_str(&format!("{dim}[tools]{dim:#}\n"));
        for line in wrapped {
            out.push_str(&format!("{indent}{line}\n"));
        }
        out.push('\n');
    }

    if !mcp_groups.is_empty() {
        let mcp_items: Vec<String> = mcp_groups
            .iter()
            .map(|(server, count)| format!("{server} ({count} tool{})", if *count == 1 { "" } else { "s" }))
            .collect();
        let text = mcp_items.join(", ");
        let wrapped = crate::ui::interactive::layout::wrap_to_width(&text, wrap_width);
        out.push_str(&format!("{dim}[mcp]{dim:#}\n"));
        for line in wrapped {
            out.push_str(&format!("{indent}{line}\n"));
        }
        out.push('\n');
    }

    if !welcome.plugins.is_empty() {
        let text = welcome.plugins.join(", ");
        let wrapped = crate::ui::interactive::layout::wrap_to_width(&text, wrap_width);
        out.push_str(&format!("{dim}[plugins]{dim:#}\n"));
        for line in wrapped {
            out.push_str(&format!("{indent}{line}\n"));
        }
        out.push('\n');
    }

    out
}

pub fn render_transcript_item(input: TranscriptRenderInput<'_>) -> String {
    let width = input.width.max(20);
    let theme = input.theme;
    match input.item {
        TranscriptItem::Welcome(welcome) => format_welcome_content(welcome, theme),
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
