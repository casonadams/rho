use crate::ui::block::BlockFormat;
use crate::ui::render::{
    fetch_content_kind, format_duration_ms, format_edit_diff, format_thinking_block, format_tool_args_summary,
    format_write_preview, read_summary_parts, tool_title_style,
};
use crate::ui::theme::Theme;

pub const OSC133_ZONE_START: &str = "\x1b]133;A\x07";
pub const OSC133_ZONE_END: &str = "\x1b]133;B\x07";
pub const OSC133_ZONE_FINAL: &str = "\x1b]133;C\x07";

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
    pub hide_thinking: bool,
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
            "search" => {
                if !builtins.contains(&"search") {
                    builtins.push("search");
                }
            }
            "fetch" => {
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
            if let Some((skill_name, skill_content, user_msg)) = parse_skill_block(text) {
                let skill_tag = anstyle::Style::new()
                    .fg_color(Some(anstyle::AnsiColor::Magenta.into()))
                    .effects(anstyle::Effects::BOLD);
                let skill_block_text = if input.tools_expanded {
                    format!("{skill_tag}[skill]{skill_tag:#} **{skill_name}**\n\n{skill_content}")
                } else {
                    let dim = theme.dimmed;
                    format!("{skill_tag}[skill]{skill_tag:#} {skill_name} {dim}(ctrl+o to expand){dim:#}")
                };
                let skill_formatted = BlockFormat::new(theme.tool_success_bg, width)
                    .with_vertical_padding()
                    .render_styled(&skill_block_text);
                let user_trimmed = user_msg.trim();
                if user_trimmed.is_empty() {
                    format!("\n{skill_formatted}")
                } else {
                    let user_formatted = BlockFormat::new(theme.user_message_bg, width)
                        .with_vertical_padding()
                        .render_plain(user_trimmed);
                    format!("\n{skill_formatted}\n{user_formatted}")
                }
            } else {
                let block = BlockFormat::new(theme.user_message_bg, width)
                    .with_vertical_padding()
                    .render_plain(text);
                format!("\n{block}")
            }
        }
        TranscriptItem::AssistantText(text) => {
            let mut md = crate::ui::markdown::MarkdownRenderer::default();
            let rendered = md.render_token(text, theme);
            let flushed = md.flush(theme);
            let full = format!("{rendered}{flushed}");
            if full.is_empty() {
                full
            } else {
                format!("{OSC133_ZONE_START}{full}{OSC133_ZONE_END}{OSC133_ZONE_FINAL}")
            }
        }
        TranscriptItem::Thinking(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                String::new()
            } else if input.hide_thinking {
                let dim = theme.dimmed;
                format!("{dim}Thinking...{dim:#}\n")
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
                let range_suffix = range.map_or_else(String::new, |range| {
                    let range_style = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Yellow.into()));
                    format!("{range_style}{range}{range_style:#}")
                });
                let expand_hint = if !input.tools_expanded {
                    let dim = theme.dimmed;
                    format!(" {dim}(ctrl+o to expand){dim:#}")
                } else {
                    String::new()
                };

                match rho_harness_core::presentation::summary::classify_read_path(&tool.arguments) {
                    Some(rho_harness_core::presentation::summary::ReadClassification::Skill { name }) => {
                        let skill_tag = anstyle::Style::new()
                            .fg_color(Some(anstyle::AnsiColor::Magenta.into()))
                            .effects(anstyle::Effects::BOLD);
                        format!("{skill_tag}[skill]{skill_tag:#} {name}{range_suffix}{expand_hint}")
                    }
                    Some(rho_harness_core::presentation::summary::ReadClassification::Resource { path }) => {
                        format!("{title}read resource{title:#} {accent}{path}{accent:#}{range_suffix}{expand_hint}")
                    }
                    Some(rho_harness_core::presentation::summary::ReadClassification::Docs { path }) => {
                        format!("{title}read docs{title:#} {accent}{path}{accent:#}{range_suffix}{expand_hint}")
                    }
                    None => {
                        format!("{title}read{title:#} {accent}{path}{accent:#}{range_suffix}{expand_hint}")
                    }
                }
            } else if display_name == "fetch" && !tool.is_error {
                let url = tool
                    .arguments
                    .get("url")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let status = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Yellow.into()));
                let kind = fetch_content_kind(&tool.arguments);
                format!("{title}fetch{title:#} {accent}{url}{accent:#}\n{status}fetched ({kind}){status:#}")
            } else {
                format!("{title}{display_name}{title:#} {accent}{summary}{accent:#}")
            };

            if !tool.is_error && tool.name == "read" && input.tools_expanded {
                let raw_output = if !tool.output.is_empty() {
                    &tool.output
                } else {
                    &tool.output_summary
                };
                let clean = raw_output.trim_end();
                if !clean.is_empty() {
                    content.push_str("\n\n");
                    content.push_str(clean);
                }
            } else if !tool.is_error && tool.name == "edit" {
                if let Some(diff) = format_edit_diff(&tool.arguments, theme) {
                    content.push('\n');
                    content.push_str(&diff);
                }
            } else if !tool.is_error && tool.name == "write" {
                if let Some(preview) = format_write_preview(&tool.arguments, theme) {
                    content.push('\n');
                    content.push_str(&preview);
                }
            } else if tool.name == "bash"
                || tool.is_error
                || (input.tools_expanded && tool.name != "edit" && tool.name != "write")
            {
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

fn parse_skill_block(text: &str) -> Option<(String, String, String)> {
    let start_tag = "<skill";
    let start_idx = text.find(start_tag)?;
    let name_prefix = "name=\"";
    let name_start = text[start_idx..].find(name_prefix)? + start_idx + name_prefix.len();
    let name_end = name_start + text[name_start..].find('"')?;
    let skill_name = &text[name_start..name_end];

    let content_start = start_idx + text[start_idx..].find('>')? + 1;
    let end_tag = "</skill>";
    let end_idx = text[content_start..].find(end_tag)? + content_start;
    let skill_content = &text[content_start..end_idx];

    let user_msg = &text[end_idx + end_tag.len()..];
    let user_msg = user_msg.trim_start_matches("\n\n").trim_start_matches("Skill input: ");

    Some((
        skill_name.to_string(),
        skill_content.trim().to_string(),
        user_msg.to_string(),
    ))
}
