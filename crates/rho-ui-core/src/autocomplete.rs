use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use serde::{Deserialize, Serialize};
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlashArgumentType {
    None,
    Model,
    Skill,
    FilePath,
    ThinkingLevel,
    AuthProvider,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlashCommandDef {
    pub name: &'static str,
    pub description: &'static str,
    pub arg_type: SlashArgumentType,
}

pub const BUILTIN_SLASH_COMMANDS: &[SlashCommandDef] = &[
    SlashCommandDef {
        name: "/help",
        description: "Show available commands and keyboard shortcuts",
        arg_type: SlashArgumentType::None,
    },
    SlashCommandDef {
        name: "/model",
        description: "Select active model or switch dynamically",
        arg_type: SlashArgumentType::Model,
    },
    SlashCommandDef {
        name: "/thinking",
        description: "Set reasoning effort level",
        arg_type: SlashArgumentType::ThinkingLevel,
    },
    SlashCommandDef {
        name: "/skill",
        description: "Execute or inspect installed skill",
        arg_type: SlashArgumentType::Skill,
    },
    SlashCommandDef {
        name: "/login",
        description: "Authenticate provider credentials",
        arg_type: SlashArgumentType::AuthProvider,
    },
    SlashCommandDef {
        name: "/logout",
        description: "Remove saved provider credentials",
        arg_type: SlashArgumentType::AuthProvider,
    },
    SlashCommandDef {
        name: "/mcp",
        description: "Manage MCP server connections and tools",
        arg_type: SlashArgumentType::None,
    },
    SlashCommandDef {
        name: "/tree",
        description: "Explore and branch conversation history",
        arg_type: SlashArgumentType::None,
    },
    SlashCommandDef {
        name: "/settings",
        description: "Configure thinking visibility, tools, and Vim mode",
        arg_type: SlashArgumentType::None,
    },
    SlashCommandDef {
        name: "/clear",
        description: "Reset active conversation turns",
        arg_type: SlashArgumentType::None,
    },
];

pub const THINKING_LEVEL_OPTIONS: &[(&str, &str)] = &[
    ("off", "Disable reasoning tokens"),
    ("low", "Quick light reasoning"),
    ("medium", "Standard balanced reasoning"),
    ("high", "Deep thorough reasoning"),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutocompleteCandidate {
    pub value: String,
    pub display: String,
    pub description: Option<String>,
    pub score: i64,
    pub replacement: Range<usize>,
}

#[derive(Debug, Default)]
pub struct CompletionEngine {
    pub commands: Vec<SlashCommandDef>,
    pub models: Vec<(String, String)>,
    pub skills: Vec<(String, String)>,
    pub files: Vec<String>,
    pub auth_providers: Vec<String>,
}

impl CompletionEngine {
    pub fn new() -> Self {
        Self {
            commands: BUILTIN_SLASH_COMMANDS.to_vec(),
            models: Vec::new(),
            skills: Vec::new(),
            files: Vec::new(),
            auth_providers: vec![
                "anthropic".to_string(),
                "openai".to_string(),
                "gemini".to_string(),
                "openrouter".to_string(),
                "ollama".to_string(),
                "groq".to_string(),
            ],
        }
    }

    pub fn complete(&self, input: &str, cursor: usize) -> Vec<AutocompleteCandidate> {
        let Some(prefix) = input.get(..cursor) else {
            return Vec::new();
        };

        if let Some(candidates) = self.complete_file_mention(prefix, cursor) {
            return candidates;
        }

        if let Some(candidates) = self.complete_command_args(prefix, cursor) {
            return candidates;
        }

        self.complete_slash_commands(prefix, cursor)
    }

    fn complete_slash_commands(&self, prefix: &str, cursor: usize) -> Vec<AutocompleteCandidate> {
        if !prefix.starts_with('/') || prefix.contains(' ') {
            return Vec::new();
        }

        let query = prefix.trim_start_matches('/');
        let matcher = SkimMatcherV2::default();
        let mut candidates = Vec::new();

        for cmd in &self.commands {
            let cmd_name = cmd.name.trim_start_matches('/');
            let score = if query.is_empty() {
                Some(0)
            } else {
                matcher.fuzzy_match(cmd_name, query)
            };

            if let Some(s) = score {
                candidates.push(AutocompleteCandidate {
                    value: cmd.name.to_string(),
                    display: cmd.name.to_string(),
                    description: Some(cmd.description.to_string()),
                    score: s,
                    replacement: 0..cursor,
                });
            }
        }

        candidates.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.value.cmp(&b.value)));
        candidates
    }

    fn complete_command_args(&self, prefix: &str, cursor: usize) -> Option<Vec<AutocompleteCandidate>> {
        let space_idx = prefix.find(' ')?;
        let cmd_part = &prefix[..space_idx];
        let arg_part = &prefix[space_idx + 1..];

        let cmd = self.commands.iter().find(|c| c.name == cmd_part)?;

        let items: Vec<(String, Option<String>)> = match cmd.arg_type {
            SlashArgumentType::None => return Some(Vec::new()),
            SlashArgumentType::ThinkingLevel => THINKING_LEVEL_OPTIONS
                .iter()
                .map(|(l, d)| (l.to_string(), Some(d.to_string())))
                .collect(),
            SlashArgumentType::Model => self
                .models
                .iter()
                .map(|(id, d)| (id.clone(), Some(d.clone())))
                .collect(),
            SlashArgumentType::Skill => self.skills.iter().map(|(n, d)| (n.clone(), Some(d.clone()))).collect(),
            SlashArgumentType::AuthProvider => self.auth_providers.iter().map(|p| (p.clone(), None)).collect(),
            SlashArgumentType::FilePath => self.files.iter().map(|f| (f.clone(), None)).collect(),
        };

        Some(match_candidates(cmd_part, arg_part, items, cursor))
    }

    fn complete_file_mention(&self, prefix: &str, cursor: usize) -> Option<Vec<AutocompleteCandidate>> {
        let at_idx = prefix.rfind('@')?;
        let at_is_start = at_idx == 0 || prefix[..at_idx].ends_with(char::is_whitespace);
        if !at_is_start {
            return None;
        }

        let query = &prefix[at_idx + 1..];
        let matcher = SkimMatcherV2::default();
        let mut candidates = Vec::new();

        for file in &self.files {
            let score = if query.is_empty() {
                Some(0)
            } else {
                matcher.fuzzy_match(file, query)
            };

            if let Some(s) = score {
                candidates.push(AutocompleteCandidate {
                    value: file.clone(),
                    display: file.clone(),
                    description: None,
                    score: s,
                    replacement: at_idx..cursor,
                });
            }
        }

        candidates.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.value.cmp(&b.value)));
        candidates.truncate(25);
        Some(candidates)
    }
}

fn match_candidates(
    cmd_part: &str,
    arg_part: &str,
    items: Vec<(String, Option<String>)>,
    cursor: usize,
) -> Vec<AutocompleteCandidate> {
    let matcher = SkimMatcherV2::default();
    let mut candidates = Vec::new();
    for (value, desc) in items {
        let score = if arg_part.is_empty() {
            Some(0)
        } else {
            matcher.fuzzy_match(&value, arg_part)
        };
        if let Some(s) = score {
            candidates.push(AutocompleteCandidate {
                value: format!("{cmd_part} {value}"),
                display: value,
                description: desc,
                score: s,
                replacement: 0..cursor,
            });
        }
    }
    candidates.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.value.cmp(&b.value)));
    candidates
}

#[derive(Debug, Default, Clone)]
pub struct PromptHistory {
    entries: Vec<String>,
    cursor: usize,
    saved_draft: String,
}

impl PromptHistory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_entries(entries: Vec<String>) -> Self {
        let cursor = entries.len();
        Self {
            entries,
            cursor,
            saved_draft: String::new(),
        }
    }

    pub fn record(&mut self, prompt: &str) {
        let trimmed = prompt.trim();
        if trimmed.is_empty() {
            return;
        }
        if self.entries.last().map(|s| s.as_str()) == Some(trimmed) {
            self.cursor = self.entries.len();
            return;
        }
        self.entries.push(trimmed.to_string());
        self.cursor = self.entries.len();
    }

    pub fn previous(&mut self, current_draft: &str) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }
        if self.cursor == self.entries.len() {
            self.saved_draft = current_draft.to_string();
        }
        if self.cursor > 0 {
            self.cursor -= 1;
            Some(&self.entries[self.cursor])
        } else {
            Some(&self.entries[0])
        }
    }

    pub fn next_entry(&mut self) -> Option<&str> {
        if self.entries.is_empty() || self.cursor >= self.entries.len() {
            return None;
        }
        self.cursor += 1;
        if self.cursor < self.entries.len() {
            Some(&self.entries[self.cursor])
        } else {
            Some(&self.saved_draft)
        }
    }

    pub fn reset_cursor(&mut self) {
        self.cursor = self.entries.len();
        self.saved_draft.clear();
    }
}
