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

    // 3. Discover from project workspace dir: .rho/agents/*.md and prompts/agents/*.md
    scan_agent_dir(&workspace_dir.join(".rho/agents"), &mut templates);
    scan_agent_dir(&workspace_dir.join("prompts/agents"), &mut templates);

    // 4. Merge config table overrides: [subagents.agents.<name>]
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
                let (description, system_prompt) = parse_agent_markdown(&content);
                let template = AgentTemplate {
                    name: file_stem.to_string(),
                    description,
                    system_prompt,
                    tools: vec!["read".to_string(), "bash".to_string()],
                    model: None,
                };
                templates.insert(file_stem.to_lowercase(), template);
            }
        }
    }
}

fn parse_agent_markdown(content: &str) -> (String, String) {
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
    (description, system_prompt.trim().to_string())
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
