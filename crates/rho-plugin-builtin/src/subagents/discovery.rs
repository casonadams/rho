use super::types::AgentTemplate;
use rho_core::config::Config;
use std::collections::BTreeMap;
use std::path::Path;

pub fn builtin_templates() -> Vec<AgentTemplate> {
    vec![
        AgentTemplate {
            name: "explore".to_string(),
            description: "Fast read-only search agent for locating code and answering structural questions."
                .to_string(),
            system_prompt: "You are a fast, read-only code exploration agent. Inspect files, search keywords, and synthesize concise findings.".to_string(),
            tools: vec![
                "read".to_string(),
                "bash".to_string(),
                "websearch".to_string(),
                "webfetch".to_string(),
            ],
            model: Some("haiku".to_string()),
        },
        AgentTemplate {
            name: "plan".to_string(),
            description: "Software architect agent for designing implementation plans and breakdown slices."
                .to_string(),
            system_prompt: "You are an architect planning agent. Break complex features into vertical slices with explicit acceptance criteria.".to_string(),
            tools: vec!["read".to_string(), "bash".to_string()],
            model: None,
        },
        AgentTemplate {
            name: "general-purpose".to_string(),
            description: "General-purpose agent with full tool access for autonomous multi-step execution."
                .to_string(),
            system_prompt: "You are an autonomous general-purpose coding agent. Complete the requested objective and report concise outcomes.".to_string(),
            tools: vec![
                "read".to_string(),
                "write".to_string(),
                "edit".to_string(),
                "bash".to_string(),
                "websearch".to_string(),
                "webfetch".to_string(),
                "todo".to_string(),
            ],
            model: None,
        },
    ]
}

pub fn discover_templates(config: &Config, workspace_dir: &Path) -> BTreeMap<String, AgentTemplate> {
    let mut templates = BTreeMap::new();

    // 1. Built-in defaults
    for t in builtin_templates() {
        templates.insert(t.name.to_lowercase(), t);
    }

    // 2. Discover from user config dir: ~/.config/rho/agents/*.md
    scan_agent_dir(&config.config_dir.join("agents"), &mut templates);

    // 3. Discover from project workspace dir: .rho/agents/*.md, .pi/agents/*.md, prompts/agents/*.md
    scan_agent_dir(&workspace_dir.join(".rho/agents"), &mut templates);
    scan_agent_dir(&workspace_dir.join(".pi/agents"), &mut templates);
    scan_agent_dir(&workspace_dir.join("prompts/agents"), &mut templates);

    // 4. Discover from global ~/.pi/agent/agents
    if let Ok(home) = std::env::var("HOME") {
        scan_agent_dir(&Path::new(&home).join(".pi/agent/agents"), &mut templates);
    }

    // 5. Merge config table overrides: [subagents.agents.<name>]
    for (name, def) in &config.subagents.agents {
        let entry = templates.entry(name.to_lowercase()).or_insert_with(|| AgentTemplate {
            name: name.clone(),
            description: def.description.clone(),
            system_prompt: def.system_prompt.clone().unwrap_or_default(),
            tools: def.tools.clone(),
            model: def.model.clone(),
        });
        if !def.description.is_empty() {
            entry.description = def.description.clone();
        }
        if let Some(prompt) = &def.system_prompt {
            entry.system_prompt = prompt.clone();
        }
        if !def.tools.is_empty() {
            entry.tools = def.tools.clone();
        }
        if def.model.is_some() {
            entry.model = def.model.clone();
        }
    }

    templates
}

fn scan_agent_dir(dir: &Path, templates: &mut BTreeMap<String, AgentTemplate>) {
    if !dir.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            let Some(file_stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if let Ok(content) = std::fs::read_to_string(&path) {
                let template = parse_agent_markdown(file_stem, &content);
                templates.insert(file_stem.to_lowercase(), template);
            }
        }
    }
}

pub fn parse_agent_markdown(file_stem: &str, content: &str) -> AgentTemplate {
    let trimmed = content.trim_start();
    if let Some(after_first) = trimmed.strip_prefix("---")
        && let Some(end_idx) = after_first.find("\n---")
    {
        let frontmatter = &after_first[..end_idx];
        let body = after_first[end_idx + 4..].trim();

        let mut name = file_stem.to_string();
        let mut description = String::new();
        let mut model = None;
        let mut tools = Vec::new();

        for line in frontmatter.lines() {
            let line = line.trim();
            if let Some((k, v)) = line.split_once(':') {
                let key = k.trim().to_lowercase();
                let val = v.trim().trim_matches('"').trim_matches('\'').to_string();
                match key.as_str() {
                    "name" => {
                        if !val.is_empty() {
                            name = val;
                        }
                    }
                    "description" => description = val,
                    "model" => {
                        if !val.is_empty() {
                            model = Some(val);
                        }
                    }
                    "tools" => {
                        let tool_parts: Vec<String> = val
                            .trim_matches(|c| c == '[' || c == ']')
                            .split(',')
                            .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        tools.extend(tool_parts);
                    }
                    _ => {}
                }
            }
        }

        if tools.is_empty() {
            tools = vec!["read".to_string(), "bash".to_string()];
        }

        return AgentTemplate {
            name,
            description,
            system_prompt: body.to_string(),
            tools,
            model,
        };
    }

    // Fallback: simple markdown parsing
    let mut lines = content.lines();
    let mut description = String::new();
    let mut system_prompt = String::new();

    if let Some(first_line) = lines.next() {
        let trimmed = first_line.trim().trim_start_matches('#').trim();
        description = trimmed.to_string();
    }
    for line in lines {
        system_prompt.push_str(line);
        system_prompt.push('\n');
    }
    AgentTemplate {
        name: file_stem.to_string(),
        description,
        system_prompt: system_prompt.trim().to_string(),
        tools: vec!["read".to_string(), "bash".to_string()],
        model: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_templates() {
        let templates = builtin_templates();
        assert!(templates.iter().any(|t| t.name == "explore"));
        assert!(templates.iter().any(|t| t.name == "plan"));
        assert!(templates.iter().any(|t| t.name == "general-purpose"));
    }

    #[test]
    fn test_parse_frontmatter_markdown() {
        let md = r#"---
name: codebase-locator
model: haiku
description: Locates files and components
tools: read, bash, websearch
---

You are a codebase locator agent.
"#;
        let template = parse_agent_markdown("codebase-locator", md);
        assert_eq!(template.name, "codebase-locator");
        assert_eq!(template.model.as_deref(), Some("haiku"));
        assert_eq!(template.description, "Locates files and components");
        assert_eq!(template.tools, vec!["read", "bash", "websearch"]);
        assert_eq!(template.system_prompt, "You are a codebase locator agent.");
    }

    #[test]
    fn test_discover_templates_with_config_overrides() {
        let mut config = Config::default();
        config.subagents.agents.insert(
            "custom-audit".to_string(),
            rho_core::config::AgentDefinition {
                description: "Security auditor".to_string(),
                system_prompt: Some("Audit code for vulnerabilities.".to_string()),
                tools: vec!["read".to_string()],
                model: Some("claude-3-7-sonnet".to_string()),
            },
        );

        let discovered = discover_templates(&config, Path::new("."));
        assert!(discovered.contains_key("custom-audit"));
        assert_eq!(discovered["custom-audit"].description, "Security auditor");
        assert_eq!(discovered["custom-audit"].tools, vec!["read"]);
    }
}
