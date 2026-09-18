use std::collections::BTreeMap;

use crate::ui::interactive::layout::{truncate_to_visual_lines, wrap_words_to_width};
use crate::ui::render::{
    fetch_content_kind, format_bash_args_header, format_duration_ms, format_edit_diff, format_read_expanded,
    format_thinking_block, format_tool_args_summary, format_write_preview, read_summary_parts,
};
use crate::ui::theme::{BlockStyle, Theme};

#[cfg(test)]
#[path = "transcript_tests.rs"]
mod tests;

pub const OSC133_ZONE_START: &str = "\x1b]133;A\x07";
pub const OSC133_ZONE_END: &str = "\x1b]133;B\x07";
pub const OSC133_ZONE_FINAL: &str = "\x1b]133;C\x07";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WelcomeItem {
    pub version: String,
    pub model: String,
    pub provider: String,
    pub resumed: bool,
    pub location: String,
    pub agents: Vec<String>,
    pub tools: Vec<String>,
    pub skills: Vec<String>,
    pub mcp: Vec<String>,
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

pub fn render_transcript_item(mut input: TranscriptRenderInput<'_>) -> String {
    input.width = input.width.max(20);
    match input.item {
        TranscriptItem::Welcome(welcome) => format_welcome_content(welcome, input.width, input.theme),
        TranscriptItem::UserMessage(text) => render_user_message(text, &input),
        TranscriptItem::AssistantText(text) => render_assistant_text(text, input.width, input.theme),
        TranscriptItem::Thinking(text) => render_thinking_text(text, input.width, input.hide_thinking, input.theme),
        TranscriptItem::Tool(tool) => render_tool_transcript(tool, &input),
        TranscriptItem::Notice(text) => text.clone(),
    }
}

fn render_assistant_text(text: &str, width: usize, theme: &Theme) -> String {
    let mut md = crate::ui::markdown::MarkdownRenderer::default();
    let render_width = if theme.block_agent_output {
        theme.agent_block(width).inner_width()
    } else {
        width
    };
    md.set_width(render_width);
    let full = md.render_text(text, theme);
    let trimmed = full.trim();
    if trimmed.is_empty() {
        String::new()
    } else if theme.block_agent_output {
        let block = theme.agent_block(width).with_vertical_padding().render_styled(trimmed);
        format!("{OSC133_ZONE_START}\n{block}{OSC133_ZONE_END}{OSC133_ZONE_FINAL}")
    } else {
        format!("{OSC133_ZONE_START}\n{full}{OSC133_ZONE_END}{OSC133_ZONE_FINAL}")
    }
}

fn render_thinking_text(text: &str, width: usize, hide_thinking: bool, theme: &Theme) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        String::new()
    } else if hide_thinking {
        let dim = theme.dimmed;
        format!("\n{dim}Thinking...{dim:#}\n")
    } else {
        format_thinking_block(trimmed, theme, width)
    }
}

pub fn render_user_message(text: &str, input: &TranscriptRenderInput<'_>) -> String {
    if let Some((skill_name, skill_content, user_msg)) = parse_skill_block(text) {
        render_parsed_skill(&skill_name, &skill_content, &user_msg, input)
    } else {
        let block = input
            .theme
            .user_block(input.width)
            .with_vertical_padding()
            .render_plain(text);
        if input.theme.block_style == BlockStyle::Border {
            block
        } else {
            format!("\n{block}")
        }
    }
}

fn render_parsed_skill(
    skill_name: &str,
    skill_content: &str,
    user_msg: &str,
    input: &TranscriptRenderInput<'_>,
) -> String {
    let skill_tag = input.theme.skill_tag;
    let skill_block_text = if input.tools_expanded {
        format!("{skill_tag}[skill]{skill_tag:#} **{skill_name}**\n\n{skill_content}")
    } else {
        format!("{skill_tag}[skill]{skill_tag:#} {skill_name}")
    };
    let skill_formatted = input
        .theme
        .agent_block(input.width)
        .with_vertical_padding()
        .render_styled(&skill_block_text);
    let user_trimmed = user_msg.trim();
    if user_trimmed.is_empty() {
        if input.theme.block_style == BlockStyle::Border {
            skill_formatted
        } else {
            format!("\n{skill_formatted}")
        }
    } else {
        let user_formatted = input
            .theme
            .user_block(input.width)
            .with_vertical_padding()
            .render_plain(user_trimmed);
        if input.theme.block_style == BlockStyle::Border {
            format!("{skill_formatted}{user_formatted}")
        } else {
            format!("\n{skill_formatted}\n{user_formatted}")
        }
    }
}

pub fn parse_skill_block(text: &str) -> Option<(String, String, String)> {
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

fn append_welcome_section(out: &mut String, title: &str, items: &[String], width: usize, dim: anstyle::Style) {
    if items.is_empty() {
        return;
    }
    let text = items.join(", ");
    let max_w = width.saturating_sub(4).max(20);
    let wrapped = wrap_words_to_width(&text, max_w);
    out.push_str(&format!("{dim}[{title}]{dim:#}\n"));
    for line in wrapped {
        out.push_str(&format!("  {line}\n"));
    }
    out.push('\n');
}

struct ToolCategories {
    builtins: Vec<String>,
    mcp_groups: BTreeMap<String, usize>,
    custom: Vec<String>,
}

fn push_unique(list: &mut Vec<String>, item: &str) {
    if !list.iter().any(|s| s == item) {
        list.push(item.to_string());
    }
}

fn classify_tools(tools: &[String]) -> ToolCategories {
    let mut builtins = Vec::new();
    let mut mcp_groups = BTreeMap::new();
    let mut custom = Vec::new();

    for tool in tools {
        match tool.as_str() {
            "fd" | "read" | "rg" | "write" | "edit" | "bash" => push_unique(&mut builtins, tool),
            "search" | "web_search" => push_unique(&mut builtins, "web_search"),
            "fetch" | "web_fetch" => push_unique(&mut builtins, "web_fetch"),
            "mcp" | "mcpScript" => {}
            other => {
                if let Some((server, _)) = other.split_once('_') {
                    *mcp_groups.entry(server.to_string()).or_default() += 1;
                } else {
                    push_unique(&mut custom, other);
                }
            }
        }
    }
    ToolCategories {
        builtins,
        mcp_groups,
        custom,
    }
}

fn format_mcp_items(configured: &[String], mcp_groups: &BTreeMap<String, usize>) -> Vec<String> {
    let mut items = Vec::new();
    for server in configured {
        if let Some(count) = mcp_groups.get(server) {
            items.push(format!("{server} ({count} tool{})", if *count == 1 { "" } else { "s" }));
        } else {
            items.push(server.clone());
        }
    }
    for (server, count) in mcp_groups {
        if !configured.contains(server) {
            items.push(format!("{server} ({count} tool{})", if *count == 1 { "" } else { "s" }));
        }
    }
    items
}

pub fn format_welcome_content(welcome: &WelcomeItem, width: usize, theme: &Theme) -> String {
    let (highlight, dim) = (theme.highlight, theme.dimmed);
    let mut out = format!(
        "\n{highlight}rho{highlight:#} {dim}v{}{dim:#}\n{dim}Type /help for commands, Tab to complete, Esc to cancel{dim:#}\n\n",
        welcome.version
    );
    append_welcome_section(&mut out, "agents", &welcome.agents, width, dim);
    append_welcome_section(&mut out, "skills", &welcome.skills, width, dim);

    let tools = classify_tools(&welcome.tools);
    let mut all_tools = tools.builtins;
    all_tools.extend(tools.custom);
    append_welcome_section(&mut out, "tools", &all_tools, width, dim);

    let mcp = format_mcp_items(&welcome.mcp, &tools.mcp_groups);
    append_welcome_section(&mut out, "mcp", &mcp, width, dim);
    out
}

fn format_read_header(tool: &ToolItem, theme: &Theme) -> String {
    let (path, range) = read_summary_parts(&tool.arguments);
    let title = theme.tool_title_style(false);
    let accent = theme.highlight;
    let range_suffix = range.map_or_else(String::new, |range| {
        let rs = theme.warning;
        format!("{rs}{range}{rs:#}")
    });
    match rho_harness_core::presentation::summary::classify_read_path(&tool.arguments) {
        Some(rho_harness_core::presentation::summary::ReadClassification::Skill { name }) => {
            let tag = theme.skill_tag;
            format!("{tag}[skill]{tag:#} {name}{range_suffix}")
        }
        Some(rho_harness_core::presentation::summary::ReadClassification::Resource { path }) => {
            format!("{title}read resource{title:#} {accent}{path}{accent:#}{range_suffix}")
        }
        Some(rho_harness_core::presentation::summary::ReadClassification::Docs { path }) => {
            format!("{title}read docs{title:#} {accent}{path}{accent:#}{range_suffix}")
        }
        None => format!("{title}read{title:#} {accent}{path}{accent:#}{range_suffix}"),
    }
}

fn format_tool_header(tool: &ToolItem, theme: &Theme) -> String {
    let title = theme.tool_title_style(tool.is_error);
    let accent = theme.highlight;
    let display_name = match tool.name.as_str() {
        "search" | "websearch" => "web_search",
        "fetch" | "webfetch" => "web_fetch",
        other => other,
    };
    if !tool.is_error && tool.name == "read" {
        return format_read_header(tool, theme);
    }
    if !tool.is_error && display_name == "web_fetch" {
        let url = tool
            .arguments
            .get("url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let status = theme.warning;
        let kind = fetch_content_kind(&tool.arguments);
        return format!("{title}web_fetch{title:#} {accent}{url}{accent:#}\n{status}fetched ({kind}){status:#}");
    }
    let summary = format_tool_args_summary(&tool.name, &tool.arguments);
    if summary.is_empty() {
        format!("{title}{display_name}{title:#}")
    } else {
        let header_args = if tool.name == "bash" {
            format_bash_args_header(&summary, accent, theme.dimmed)
        } else {
            format!("{accent}{summary}{accent:#}")
        };
        format!("{title}{display_name}{title:#} {header_args}")
    }
}

fn append_read_expanded(content: &mut String, tool: &ToolItem, theme: &Theme) {
    let raw = if !tool.output.is_empty() {
        &tool.output
    } else {
        &tool.output_summary
    };
    if let Some(formatted) = format_read_expanded(raw, &tool.arguments, theme) {
        content.push_str("\n\n");
        content.push_str(&formatted);
    }
}

fn append_output_lines(content: &mut String, clean: &str, width: usize, expanded: bool, dim: anstyle::Style) {
    content.push_str("\n\n");
    if expanded {
        content.push_str(clean);
    } else {
        let truncated = truncate_to_visual_lines(clean, 5, width.saturating_sub(4).max(1));
        if truncated.skipped_count > 0 {
            content.push_str(&format!(
                "{dim}... ({n} earlier lines){dim:#}\n",
                n = truncated.skipped_count
            ));
        }
        content.push_str(&truncated.visual_lines.join("\n"));
    }
}

fn append_generic_output(content: &mut String, tool: &ToolItem, width: usize, expanded: bool, dim: anstyle::Style) {
    let raw = if !tool.output.is_empty() {
        &tool.output
    } else {
        &tool.output_summary
    };
    let clean = raw.trim_end().replace('\t', "   ");
    if !clean.is_empty() {
        append_output_lines(content, &clean, width, expanded, dim);
    }
}

fn append_edit_or_write(content: &mut String, tool: &ToolItem, expanded: bool, theme: &Theme) -> bool {
    if !tool.is_error && tool.name == "edit" {
        if let Some(diff) = format_edit_diff(&tool.arguments, theme) {
            content.push_str("\n\n");
            content.push_str(&diff);
        }
        return true;
    }
    if !tool.is_error && tool.name == "write" {
        if let Some(preview) = format_write_preview(&tool.arguments, theme, expanded) {
            content.push_str("\n\n");
            content.push_str(&preview);
        }
        return true;
    }
    false
}

fn append_tool_details(content: &mut String, tool: &ToolItem, width: usize, expanded: bool, theme: &Theme) {
    if !tool.is_error && tool.name == "read" {
        if expanded {
            append_read_expanded(content, tool, theme);
        }
    } else if !append_edit_or_write(content, tool, expanded, theme)
        && (tool.name == "bash" || tool.is_error || expanded)
    {
        append_generic_output(content, tool, width, expanded, theme.dimmed);
    }
    if tool.name == "bash"
        && let Some(duration_ms) = tool.duration_ms
    {
        let dim = theme.dimmed;
        content.push_str(&format!("\n\n{dim}Took {}{dim:#}", format_duration_ms(duration_ms)));
    }
}

pub fn render_tool_block(tool: &ToolItem, input: &TranscriptRenderInput<'_>) -> String {
    let mut content = format_tool_header(tool, input.theme);
    append_tool_details(&mut content, tool, input.width, input.tools_expanded, input.theme);

    input
        .theme
        .tool_block(tool.name == "bash", tool.is_error, input.width)
        .with_vertical_padding()
        .render_styled(&content)
}

pub fn render_tool_transcript(tool: &ToolItem, input: &TranscriptRenderInput<'_>) -> String {
    let block = render_tool_block(tool, input);
    if input.theme.block_style == BlockStyle::Border {
        block
    } else {
        format!("\n{block}")
    }
}
