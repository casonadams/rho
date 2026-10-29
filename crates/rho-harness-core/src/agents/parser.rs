use super::types::AgentMetadata;
use std::path::Path;

pub fn normalize_tool_name(name: &str) -> String {
    let lower = name.trim().to_ascii_lowercase();
    match lower.as_str() {
        "glob" => "fd".to_string(),
        "grep" => "rg".to_string(),
        "websearch" => "web_search".to_string(),
        "webfetch" => "web_fetch".to_string(),
        "agent" => "subagent".to_string(),
        _ => lower,
    }
}

pub fn parse_tool_list(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    let cleaned = if trimmed.starts_with('[') && trimmed.ends_with(']') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    cleaned
        .split(',')
        .map(|item| item.trim().trim_matches('"').trim_matches('\''))
        .filter(|item| !item.is_empty())
        .map(normalize_tool_name)
        .collect()
}

#[derive(Default)]
struct ParsedFrontmatter {
    name: Option<String>,
    description: Option<String>,
    tools: Option<Vec<String>>,
    disallowed_tools: Option<Vec<String>>,
    model: Option<String>,
    max_turns: Option<usize>,
}

fn parse_frontmatter_field(line: &str, out: &mut ParsedFrontmatter) {
    if let Some(val) = line.strip_prefix("name:") {
        out.name = Some(val.trim().trim_matches('"').trim_matches('\'').to_string());
    } else if let Some(val) = line.strip_prefix("description:") {
        out.description = Some(val.trim().trim_matches('"').trim_matches('\'').to_string());
    } else if let Some(val) = line.strip_prefix("tools:") {
        out.tools = Some(parse_tool_list(val));
    } else if let Some(val) = line
        .strip_prefix("disallowedTools:")
        .or_else(|| line.strip_prefix("disallowed_tools:"))
        .or_else(|| line.strip_prefix("disallowed-tools:"))
    {
        out.disallowed_tools = Some(parse_tool_list(val));
    } else if let Some(val) = line.strip_prefix("model:") {
        out.model = Some(val.trim().trim_matches('"').trim_matches('\'').to_string());
    } else if let Some(val) = line
        .strip_prefix("maxTurns:")
        .or_else(|| line.strip_prefix("max_turns:"))
        .or_else(|| line.strip_prefix("max-turns:"))
    {
        out.max_turns = val.trim().trim_matches('"').trim_matches('\'').parse().ok();
    }
}

pub fn parse_agent_file(path: &Path) -> Option<AgentMetadata> {
    let content = std::fs::read_to_string(path).ok()?;
    let fallback_name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("subagent")
        .to_string();

    let mut frontmatter = ParsedFrontmatter::default();
    let body = if content.starts_with("---") {
        let parts: Vec<&str> = content.splitn(3, "---").collect();
        if parts.len() >= 3 {
            for line in parts[1].lines() {
                parse_frontmatter_field(line.trim(), &mut frontmatter);
            }
            parts[2].trim().to_string()
        } else {
            content.trim().to_string()
        }
    } else {
        content.trim().to_string()
    };

    Some(AgentMetadata {
        name: frontmatter.name.unwrap_or(fallback_name),
        description: frontmatter.description.unwrap_or_else(|| "Custom subagent".to_string()),
        location: path.display().to_string(),
        tools: frontmatter.tools,
        disallowed_tools: frontmatter.disallowed_tools,
        model: frontmatter.model,
        max_turns: frontmatter.max_turns,
        system_prompt: body,
    })
}
