use std::ops::Range;

use rho_harness_core::provider::ProviderId;

use crate::repl::interactive::fuzzy::fuzzy_match;

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
    (
        "settings",
        "Configure runtime interface settings (thinking effort, display toggles)",
    ),
    ("model", "Select model (opens selector UI) <provider/model>"),
    ("resume", "Resume a previous session (opens session selector)"),
    ("skill", "List, inspect, or invoke declarative skills"),
    ("mcp", "Inspect and manage Model Context Protocol servers"),
    ("session", "Display token capacity and session diagnostics"),
    (
        "tokens",
        "Display token capacity and session diagnostics (alias for /session)",
    ),
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
    ("remote", "Pair session with web dashboard via Iroh P2P"),
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
    pub(super) commands: Vec<CommandItem>,
    pub(super) skills: Vec<SkillItem>,
    pub(super) models: Vec<ModelItem>,
    pub(super) providers: Vec<ProviderItem>,
    pub(super) files: Vec<String>,
}

fn build_command_items(sources: &super::sources::CompletionSources) -> Vec<CommandItem> {
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
    for s in &sources.skills {
        commands.push(CommandItem {
            name: format!("/skill:{}", s.metadata.name),
            description: format!("{} [{}]", s.metadata.description, s.origin),
        });
    }
    commands.sort_by(|a, b| a.name.cmp(&b.name));
    commands.dedup_by(|a, b| a.name == b.name);
    commands
}

fn build_provider_items(custom_providers: Vec<String>) -> Vec<ProviderItem> {
    let mut providers = Vec::new();
    for p in ProviderId::ALL {
        providers.push(ProviderItem {
            name: p.as_str().to_string(),
            auth_mode: p.auth_mode_label().to_string(),
        });
    }
    for name in custom_providers {
        if !providers.iter().any(|p| p.name == name) {
            providers.push(ProviderItem {
                name,
                auth_mode: "custom endpoint".to_string(),
            });
        }
    }
    providers
}

impl CompletionSet {
    pub fn from_sources(sources: super::sources::CompletionSources) -> Self {
        let commands = build_command_items(&sources);
        let skills = sources
            .skills
            .into_iter()
            .map(|s| SkillItem {
                name: s.metadata.name,
                description: s.metadata.description,
                origin: s.origin.to_string(),
            })
            .collect();
        let providers = build_provider_items(sources.custom_providers);
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

        if let Some(results) = complete_slash_args(self, prefix, cursor) {
            return results;
        }
        if let Some(results) = complete_files(&self.files, prefix, cursor) {
            return results;
        }
        complete_slash_commands(&self.commands, prefix, cursor)
    }
}

fn complete_files(files: &[String], prefix: &str, cursor: usize) -> Option<Vec<Completion>> {
    let at_idx = prefix.rfind('@')?;
    let at_is_word_start = at_idx == 0 || prefix[..at_idx].ends_with(char::is_whitespace);
    if !at_is_word_start {
        return Some(Vec::new());
    }
    let file_prefix = &prefix[at_idx + 1..];
    let lower_prefix = file_prefix.to_lowercase();
    Some(
        files
            .iter()
            .filter(|f| f.to_lowercase().contains(&lower_prefix))
            .take(25)
            .map(|f| Completion {
                value: f.clone(),
                description: None,
                replacement: at_idx..cursor,
            })
            .collect(),
    )
}

fn complete_slash_commands(commands: &[CommandItem], prefix: &str, cursor: usize) -> Vec<Completion> {
    if !prefix.starts_with('/') || prefix.contains(' ') || prefix[1..].contains('/') {
        return Vec::new();
    }
    let query = prefix.trim_start_matches('/');
    let mut scored: Vec<(i32, &CommandItem)> = commands
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
}

struct TargetArgs<'a> {
    cmd: &'a str,
    argument: &'a str,
    cursor: usize,
}

fn complete_auth_args(set: &CompletionSet, prefix: &str, cursor: usize) -> Option<Vec<Completion>> {
    if let Some(argument) = prefix.strip_prefix("/login ") {
        Some(complete_provider(
            &set.providers,
            TargetArgs {
                cmd: "/login",
                argument,
                cursor,
            },
        ))
    } else {
        prefix.strip_prefix("/logout ").map(|argument| {
            complete_provider(
                &set.providers,
                TargetArgs {
                    cmd: "/logout",
                    argument,
                    cursor,
                },
            )
        })
    }
}

pub(super) fn complete_slash_args(set: &CompletionSet, prefix: &str, cursor: usize) -> Option<Vec<Completion>> {
    if let Some(argument) = prefix
        .strip_prefix("/skill ")
        .or_else(|| prefix.strip_prefix("/skills "))
    {
        return Some(complete_skills(&set.skills, argument, cursor));
    }
    if let Some(argument) = prefix.strip_prefix("/model ") {
        return Some(complete_models(&set.models, argument, cursor));
    }
    if let Some(argument) = prefix.strip_prefix("/thinking ") {
        return Some(complete_thinking(argument, cursor));
    }
    complete_auth_args(set, prefix, cursor)
}

fn complete_skills(skills: &[SkillItem], argument: &str, cursor: usize) -> Vec<Completion> {
    let mut scored: Vec<(i32, &SkillItem)> = skills
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
}

fn complete_models(models: &[ModelItem], argument: &str, cursor: usize) -> Vec<Completion> {
    let mut scored: Vec<(i32, &ModelItem)> = models
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
}

fn complete_provider(providers: &[ProviderItem], target: TargetArgs<'_>) -> Vec<Completion> {
    let mut scored: Vec<(i32, &ProviderItem)> = providers
        .iter()
        .filter_map(|p| {
            if target.argument.is_empty() {
                Some((0, p))
            } else {
                fuzzy_match(target.argument, &p.name).map(|score| (score, p))
            }
        })
        .collect();
    scored.sort_by_key(|(score, p)| (*score, p.name.clone()));

    scored
        .into_iter()
        .map(|(_, p)| Completion {
            value: format!("{} {}", target.cmd, p.name),
            description: Some(p.auth_mode.clone()),
            replacement: 0..target.cursor,
        })
        .collect()
}

fn complete_thinking(argument: &str, cursor: usize) -> Vec<Completion> {
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
    scored.sort_by_key(|(score, lvl)| (*score, lvl.0));

    scored
        .into_iter()
        .map(|(_, (level, desc))| Completion {
            value: format!("/thinking {level}"),
            description: Some((*desc).to_string()),
            replacement: 0..cursor,
        })
        .collect()
}
