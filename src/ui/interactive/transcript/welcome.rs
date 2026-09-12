use crate::ui::interactive::layout::wrap_words_to_width;
use crate::ui::theme::Theme;
use std::collections::BTreeMap;

use super::types::WelcomeItem;

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

fn format_mcp_items(mcp_groups: &BTreeMap<String, usize>) -> Vec<String> {
    mcp_groups
        .iter()
        .map(|(server, count)| format!("{server} ({count} tool{})", if *count == 1 { "" } else { "s" }))
        .collect()
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

    let mcp = format_mcp_items(&tools.mcp_groups);
    append_welcome_section(&mut out, "mcp", &mcp, width, dim);
    out
}
