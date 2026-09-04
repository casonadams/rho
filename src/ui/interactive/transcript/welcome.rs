use crate::ui::interactive::layout::wrap_to_width;
use crate::ui::theme::Theme;
use std::collections::BTreeMap;

use super::types::WelcomeItem;

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
        let wrapped = wrap_to_width(&text, wrap_width);
        out.push_str(&format!("{dim}[skills]{dim:#}\n"));
        for line in wrapped {
            out.push_str(&format!("{indent}{line}\n"));
        }
        out.push('\n');
    }

    let mut builtins = Vec::new();
    let mut mcp_groups: BTreeMap<String, usize> = BTreeMap::new();
    let mut custom_tools = Vec::new();

    for tool in &welcome.tools {
        match tool.as_str() {
            "fd" | "read" | "rg" | "write" | "edit" | "bash" => {
                if !builtins.contains(&tool.as_str()) {
                    builtins.push(tool.as_str());
                }
            }
            "search" | "web_search" => {
                if !builtins.contains(&"web_search") {
                    builtins.push("web_search");
                }
            }
            "fetch" | "web_fetch" => {
                if !builtins.contains(&"web_fetch") {
                    builtins.push("web_fetch");
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
        let wrapped = wrap_to_width(&text, wrap_width);
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
        let wrapped = wrap_to_width(&text, wrap_width);
        out.push_str(&format!("{dim}[mcp]{dim:#}\n"));
        for line in wrapped {
            out.push_str(&format!("{indent}{line}\n"));
        }
        out.push('\n');
    }

    if !welcome.plugins.is_empty() {
        let text = welcome.plugins.join(", ");
        let wrapped = wrap_to_width(&text, wrap_width);
        out.push_str(&format!("{dim}[plugins]{dim:#}\n"));
        for line in wrapped {
            out.push_str(&format!("{indent}{line}\n"));
        }
        out.push('\n');
    }

    out
}
