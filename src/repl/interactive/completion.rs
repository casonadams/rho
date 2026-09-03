use std::ops::Range;

use super::fuzzy::fuzzy_match;
use rho_harness_core::provider::ProviderId;

pub const THINKING_LEVELS: &[(&str, &str)] = &[
    ("off", "No reasoning"),
    ("minimal", "Very brief reasoning (~1k tokens)"),
    ("low", "Light reasoning (~2k tokens)"),
    ("medium", "Moderate reasoning (~8k tokens)"),
    ("high", "Deep reasoning (~16k tokens)"),
    ("xhigh", "Extra-high reasoning (~32k tokens)"),
    ("max", "Maximum reasoning"),
];

pub const BUILTIN_SLASH_COMMANDS: &[(&str, &str)] = &[
    ("help", "Show reference of available commands and shortcuts"),
    ("settings", "Configure runtime interface settings"),
    ("model", "Select model (opens selector UI) <provider/model>"),
    ("resume", "Resume a previous session (opens session selector)"),
    ("thinking", "Set thinking level <level>"),
    ("skill", "List, inspect, or invoke declarative skills"),
    ("plugin", "Inspect configured MCP servers and plugins"),
    ("session", "Display token capacity and session diagnostics"),
    ("compact", "Manually compact the session context"),
    ("tree", "Navigate session tree (switch branches)"),
    ("fork", "Create a new fork from a previous user message"),
    ("clone", "Duplicate the current session at the current position"),
    ("name", "Set session display name"),
    ("rewind", "Rewind context to a specific prior turn"),
    ("new", "Start a new session"),
    ("clear", "Start a new session (alias for /new)"),
    ("login", "Configure provider authentication <provider>"),
    ("logout", "Remove stored provider authentication <provider>"),
    ("reload", "Reload config, skills, prompt templates, and MCP tools"),
    ("export", "Export session (HTML default, or specify path: .html/.md)"),
    ("exit", "Exit rho"),
    ("quit", "Exit rho"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandItem {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillItem {
    pub name: String,
    pub description: String,
    pub origin: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelItem {
    pub id: String,
    pub provider: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderItem {
    pub name: String,
    pub auth_mode: String,
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
    skills: Vec<SkillItem>,
    models: Vec<ModelItem>,
    providers: Vec<ProviderItem>,
    files: Vec<String>,
}

impl CompletionSet {
    pub fn from_sources(sources: super::sources::CompletionSources) -> Self {
        let mut commands = Vec::new();
        for (name, desc) in BUILTIN_SLASH_COMMANDS {
            commands.push(CommandItem {
                name: format!("/{name}"),
                description: (*desc).to_string(),
            });
        }
        for name in &sources.prompt_templates {
            commands.push(CommandItem {
                name: format!("/{name}"),
                description: "Custom prompt template".to_string(),
            });
        }
        // Register each skill directly as a top-level `/skill:<name>` command (Pi-style)
        for s in &sources.skills {
            commands.push(CommandItem {
                name: format!("/skill:{}", s.metadata.name),
                description: format!("{} [{}]", s.metadata.description, s.origin),
            });
        }
        commands.sort_by(|a, b| a.name.cmp(&b.name));
        commands.dedup_by(|a, b| a.name == b.name);

        let skills = sources
            .skills
            .into_iter()
            .map(|s| SkillItem {
                name: s.metadata.name,
                description: s.metadata.description,
                origin: s.origin.to_string(),
            })
            .collect();

        let mut providers = Vec::new();
        for p in ProviderId::ALL {
            providers.push(ProviderItem {
                name: p.as_str().to_string(),
                auth_mode: p.auth_mode_label().to_string(),
            });
        }
        for name in sources.custom_providers {
            if !providers.iter().any(|p| p.name == name) {
                providers.push(ProviderItem {
                    name,
                    auth_mode: "custom endpoint".to_string(),
                });
            }
        }

        let cwd = std::env::current_dir().ok();
        let files = cwd
            .as_deref()
            .map(|d| rho_harness_core::workspace::list_relative_files(d, 2000))
            .unwrap_or_default();

        Self {
            commands,
            skills,
            models: sources.models,
            providers,
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
            let mut scored: Vec<(i32, &SkillItem)> = self
                .skills
                .iter()
                .filter_map(|s| {
                    if argument.is_empty() {
                        Some((0, s))
                    } else {
                        fuzzy_match(argument, &s.name).map(|score| (score, s))
                    }
                })
                .collect();
            scored.sort_by_key(|(score, s)| (*score, s.name.clone()));

            scored
                .into_iter()
                .map(|(_, s)| Completion {
                    value: format!("/skill {}", s.name),
                    description: Some(format!("{} [{}]", s.description, s.origin)),
                    replacement: 0..cursor,
                })
                .collect()
        } else if let Some(argument) = prefix.strip_prefix("/model ") {
            let mut scored: Vec<(i32, &ModelItem)> = self
                .models
                .iter()
                .filter_map(|m| {
                    if argument.is_empty() {
                        Some((0, m))
                    } else {
                        let query_target = format!("{}:{}", m.provider, m.id);
                        fuzzy_match(argument, &m.id)
                            .or_else(|| fuzzy_match(argument, &query_target))
                            .map(|score| (score, m))
                    }
                })
                .collect();
            scored.sort_by_key(|(score, m)| (*score, m.id.clone()));

            scored
                .into_iter()
                .map(|(_, m)| Completion {
                    value: format!("/model {}", m.id),
                    description: Some(format!("{} · {}", m.provider, m.description)),
                    replacement: 0..cursor,
                })
                .collect()
        } else if let Some(argument) = prefix.strip_prefix("/thinking ") {
            let mut scored: Vec<(i32, &(&str, &str))> = THINKING_LEVELS
                .iter()
                .filter_map(|lvl| {
                    if argument.is_empty() {
                        Some((0, lvl))
                    } else {
                        fuzzy_match(argument, lvl.0).map(|score| (score, lvl))
                    }
                })
                .collect();
            scored.sort_by_key(|(score, lvl)| (*score, lvl.0.to_string()));

            scored
                .into_iter()
                .map(|(_, lvl)| Completion {
                    value: format!("/thinking {}", lvl.0),
                    description: Some(lvl.1.to_string()),
                    replacement: 0..cursor,
                })
                .collect()
        } else if let Some(argument) = prefix.strip_prefix("/login ") {
            let mut scored: Vec<(i32, &ProviderItem)> = self
                .providers
                .iter()
                .filter_map(|p| {
                    if argument.is_empty() {
                        Some((0, p))
                    } else {
                        fuzzy_match(argument, &p.name).map(|score| (score, p))
                    }
                })
                .collect();
            scored.sort_by_key(|(score, p)| (*score, p.name.clone()));

            scored
                .into_iter()
                .map(|(_, p)| Completion {
                    value: format!("/login {}", p.name),
                    description: Some(p.auth_mode.clone()),
                    replacement: 0..cursor,
                })
                .collect()
        } else if let Some(argument) = prefix.strip_prefix("/logout ") {
            let mut scored: Vec<(i32, &ProviderItem)> = self
                .providers
                .iter()
                .filter_map(|p| {
                    if argument.is_empty() {
                        Some((0, p))
                    } else {
                        fuzzy_match(argument, &p.name).map(|score| (score, p))
                    }
                })
                .collect();
            scored.sort_by_key(|(score, p)| (*score, p.name.clone()));

            scored
                .into_iter()
                .map(|(_, p)| Completion {
                    value: format!("/logout {}", p.name),
                    description: Some(p.auth_mode.clone()),
                    replacement: 0..cursor,
                })
                .collect()
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
