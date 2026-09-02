use std::ops::Range;

use super::fuzzy::fuzzy_match;

pub const CURATED_MODELS: &[(&str, &str)] = &[
    ("gemini-3.7-flash", "antigravity"),
    ("gpt-5.6-luna", "chatgpt"),
    ("gpt-5.4", "chatgpt"),
    ("claude-sonnet-4-6", "anthropic"),
    ("gpt-4o", "openai"),
    ("gemini-2.5-pro", "gemini"),
    ("deepseek-reasoner", "deepseek"),
];

pub const PROVIDERS: &[&str] = &[
    "antigravity",
    "anthropic",
    "openai",
    "chatgpt",
    "copilot",
    "gemini",
    "deepseek",
    "groq",
    "openrouter",
    "ollama",
];

pub const BUILTIN_SLASH_COMMANDS: &[(&str, &str)] = &[
    ("help", "Show reference of available commands and shortcuts"),
    ("model", "Inspect or switch AI model and provider"),
    ("skill", "List, inspect, or invoke declarative skills"),
    ("plugin", "Inspect configured MCP servers and plugins"),
    ("session", "Display token capacity and session diagnostics"),
    ("compact", "Summarize earlier context to free context space"),
    ("tree", "View conversation turn and branch tree"),
    ("fork", "Fork session from turn or node into a new session"),
    ("clone", "Duplicate active branch into a new session"),
    ("name", "Assign a human-readable name to the session"),
    ("rewind", "Rewind context to a specific prior turn"),
    ("clear", "Start a new session; preserve history"),
    ("login", "Add API-key or subscription authentication"),
    ("logout", "Remove stored provider authentication"),
    ("reload", "Re-read config, skills, and MCP tools; keep history"),
    ("export", "Export active branch as HTML or Markdown artifact"),
    ("exit", "Exit rho"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandItem {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub value: String,
    pub description: Option<String>,
    pub replacement: Range<usize>,
}

#[derive(Debug, Clone)]
pub struct CompletionSet {
    commands: Vec<CommandItem>,
    skills: Vec<String>,
    models: Vec<String>,
    providers: Vec<String>,
    files: Vec<String>,
}

struct MatchRequest<'a> {
    prefix: &'a str,
    leader: &'a str,
    cursor: usize,
}

impl CompletionSet {
    pub fn rho(extension_commands: &[(&str, &str)], skill_names: Vec<String>, prompt_templates: Vec<String>) -> Self {
        let mut commands = Vec::new();
        for (name, desc) in BUILTIN_SLASH_COMMANDS {
            commands.push(CommandItem {
                name: format!("/{name}"),
                description: (*desc).to_string(),
            });
        }
        for (name, desc) in extension_commands {
            commands.push(CommandItem {
                name: format!("/{name}"),
                description: (*desc).to_string(),
            });
        }
        for name in &prompt_templates {
            commands.push(CommandItem {
                name: format!("/{name}"),
                description: "Custom prompt template".to_string(),
            });
        }
        commands.sort_by(|a, b| a.name.cmp(&b.name));
        commands.dedup_by(|a, b| a.name == b.name);

        let cwd = std::env::current_dir().ok();
        let files = cwd
            .as_deref()
            .map(|d| rho_core::workspace::list_relative_files(d, 2000))
            .unwrap_or_default();

        Self {
            commands,
            skills: skill_names,
            models: CURATED_MODELS.iter().map(|(model, _)| (*model).to_string()).collect(),
            providers: PROVIDERS.iter().map(|provider| (*provider).to_string()).collect(),
            files,
        }
    }

    pub fn with_files(mut self, files: Vec<String>) -> Self {
        self.files = files;
        self
    }

    pub fn complete(&self, line: &str, cursor: usize) -> Vec<Completion> {
        let Some(prefix) = line.get(..cursor) else {
            return Vec::new();
        };

        if let Some(argument) = prefix
            .strip_prefix("/skill ")
            .or_else(|| prefix.strip_prefix("/skills "))
        {
            matches_strings(
                &self.skills,
                MatchRequest {
                    prefix: argument,
                    leader: "/skill ",
                    cursor,
                },
            )
        } else if let Some(argument) = prefix.strip_prefix("/model ") {
            matches_strings(
                &self.models,
                MatchRequest {
                    prefix: argument,
                    leader: "/model ",
                    cursor,
                },
            )
        } else if let Some(argument) = prefix.strip_prefix("/login ") {
            matches_strings(
                &self.providers,
                MatchRequest {
                    prefix: argument,
                    leader: "/login ",
                    cursor,
                },
            )
        } else if let Some(argument) = prefix.strip_prefix("/logout ") {
            matches_strings(
                &self.providers,
                MatchRequest {
                    prefix: argument,
                    leader: "/logout ",
                    cursor,
                },
            )
        } else if let Some(at_idx) = prefix.rfind('@') {
            let at_is_word_start = at_idx == 0 || prefix[..at_idx].ends_with(char::is_whitespace);
            if at_is_word_start {
                let file_prefix = &prefix[at_idx + 1..];
                self.files
                    .iter()
                    .filter(|f| f.to_lowercase().contains(&file_prefix.to_lowercase()))
                    .take(25)
                    .map(|f| Completion {
                        value: f.clone(),
                        description: None,
                        replacement: at_idx..cursor,
                    })
                    .collect()
            } else {
                Vec::new()
            }
        } else if prefix.starts_with('/') && !prefix.contains(' ') {
            let query = prefix.trim_start_matches('/');
            let mut scored: Vec<(i32, &CommandItem)> = self
                .commands
                .iter()
                .filter_map(|cmd| {
                    let cmd_name = cmd.name.trim_start_matches('/');
                    fuzzy_match(query, cmd_name).map(|score| (score, cmd))
                })
                .collect();

            scored.sort_by_key(|(score, cmd)| (*score, cmd.name.clone()));

            scored
                .into_iter()
                .map(|(_, cmd)| Completion {
                    value: cmd.name.clone(),
                    description: Some(cmd.description.clone()),
                    replacement: 0..cursor,
                })
                .collect()
        } else {
            Vec::new()
        }
    }
}

fn matches_strings(values: &[String], request: MatchRequest<'_>) -> Vec<Completion> {
    values
        .iter()
        .filter(|value| value.starts_with(request.prefix))
        .map(|value| Completion {
            value: format!("{}{value}", request.leader),
            description: None,
            replacement: 0..request.cursor,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slash_fuzzy_completion() {
        let set = CompletionSet::rho(&[], vec!["plan".to_string()], vec!["review".to_string()]);

        let slash_all = set.complete("/", 1);
        assert!(!slash_all.is_empty());
        assert!(slash_all.iter().any(|c| c.value == "/model" && c.description.is_some()));
        assert!(slash_all.iter().any(|c| c.value == "/review"));

        let mod_matches = set.complete("/mod", 4);
        assert_eq!(mod_matches[0].value, "/model");

        let exp_matches = set.complete("/exp", 4);
        assert!(exp_matches.iter().any(|c| c.value == "/export"));
    }
}
