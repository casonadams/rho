use std::ops::Range;

use crate::repl::commands::SLASH_COMMANDS;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub value: String,
    pub replacement: Range<usize>,
}

#[derive(Debug, Clone)]
pub struct CompletionSet {
    commands: Vec<String>,
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
        let mut commands = SLASH_COMMANDS
            .iter()
            .map(|command| (*command).to_string())
            .collect::<Vec<_>>();
        commands.extend(extension_commands.iter().map(|(name, _)| format!("/{name}")));
        commands.extend(prompt_templates.iter().map(|name| format!("/{name}")));
        commands.sort();
        commands.dedup();

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
            matches(
                &self.skills,
                MatchRequest {
                    prefix: argument,
                    leader: "/skill ",
                    cursor,
                },
            )
        } else if let Some(argument) = prefix.strip_prefix("/model ") {
            matches(
                &self.models,
                MatchRequest {
                    prefix: argument,
                    leader: "/model ",
                    cursor,
                },
            )
        } else if let Some(argument) = prefix.strip_prefix("/login ") {
            matches(
                &self.providers,
                MatchRequest {
                    prefix: argument,
                    leader: "/login ",
                    cursor,
                },
            )
        } else if let Some(argument) = prefix.strip_prefix("/logout ") {
            matches(
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
                        replacement: at_idx..cursor,
                    })
                    .collect()
            } else {
                Vec::new()
            }
        } else if prefix.starts_with('/') {
            matches(
                &self.commands,
                MatchRequest {
                    prefix,
                    leader: "",
                    cursor,
                },
            )
        } else {
            Vec::new()
        }
    }
}

fn matches(values: &[String], request: MatchRequest<'_>) -> Vec<Completion> {
    values
        .iter()
        .filter(|value| value.starts_with(request.prefix))
        .map(|value| Completion {
            value: format!("{}{value}", request.leader),
            replacement: 0..request.cursor,
        })
        .collect()
}
