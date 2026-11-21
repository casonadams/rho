use std::ops::Range;

use rho_harness_core::provider::ProviderId;

use crate::ui::fuzzy::fuzzy_score;
use std::cmp::Reverse;

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
    ("models", "Configure role models (default, smol, slow, guard)"),
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
    ("workflow", "List workflows, or run one: /workflow <name> [inputs]"),
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

impl ModelItem {
    pub fn canonical_id(&self) -> String {
        if self.id.starts_with(&format!("{}/", self.provider)) {
            self.id.clone()
        } else {
            format!("{}/{}", self.provider, self.id)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderItem {
    pub name: String,
    pub auth_mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerItem {
    pub name: String,
    pub target: String,
    pub oauth: bool,
}

#[derive(Debug, Clone, Default)]
pub struct McpServerList(std::sync::Arc<std::sync::RwLock<Vec<McpServerItem>>>);

impl McpServerList {
    pub fn set_from(&self, mcp: &rho_harness_core::config::McpConfig) {
        *self.0.write().unwrap() = mcp
            .servers
            .iter()
            .filter(|(_, cfg)| cfg.enabled)
            .map(|(name, cfg)| McpServerItem {
                name: name.clone(),
                target: cfg.url.clone().or_else(|| cfg.command.clone()).unwrap_or_default(),
                oauth: cfg.uses_oauth(),
            })
            .collect();
    }

    fn snapshot(&self) -> Vec<McpServerItem> {
        self.0.read().unwrap().clone()
    }
}

const MCP_SUBCOMMANDS: &[(&str, &str)] = &[
    ("login", "Sign in to a remote MCP server <server>"),
    ("logout", "Remove stored MCP sign-in <server>"),
    ("prompts", "List MCP server prompts [server]"),
    ("prompt", "Send an MCP prompt <server> <name> [key=value ...]"),
];

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
    pub(super) mcp_servers: McpServerList,
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
    for (name, description) in &sources.workflows {
        commands.push(CommandItem {
            name: format!("/workflow {name}"),
            description: format!("Workflow: {description}"),
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
            .map(|d| rho_engine::tools::list_workspace_files(d, 2000))
            .unwrap_or_default();
        Self {
            commands,
            skills,
            models: sources.models,
            providers,
            mcp_servers: sources.mcp_servers,
            files,
        }
    }

    pub fn with_files(mut self, files: Vec<String>) -> Self {
        self.files = files;
        self
    }

    pub fn with_commands(mut self, commands: Vec<CommandItem>) -> Self {
        self.commands = commands;
        self
    }

    pub fn complete(&self, line: &str, cursor: usize) -> Vec<Completion> {
        let Some(prefix) = line.get(..cursor) else {
            return Vec::new();
        };

        if let Some(results) = complete_midprompt_skills(&self.skills, prefix, cursor) {
            return results;
        }
        if let Some(results) = complete_slash_args(self, prefix, cursor) {
            return results;
        }
        if let Some(results) = complete_files(&self.files, prefix, cursor) {
            return results;
        }
        complete_slash_commands(&self.commands, prefix, cursor)
    }
}

fn complete_midprompt_skills(skills: &[SkillItem], prefix: &str, cursor: usize) -> Option<Vec<Completion>> {
    let skill_idx = prefix.rfind("/skill:")?;
    let is_word_start = skill_idx == 0 || prefix[..skill_idx].ends_with(char::is_whitespace);
    if !is_word_start {
        return None;
    }
    let query = &prefix[skill_idx + "/skill:".len()..];
    if query.contains(char::is_whitespace) {
        return None;
    }
    let mut scored: Vec<(i64, &SkillItem)> = skills
        .iter()
        .filter_map(|s| {
            if query.is_empty() {
                Some((0, s))
            } else {
                fuzzy_score(query, &s.name).map(|score| (score, s))
            }
        })
        .collect();
    scored.sort_by_key(|(score, s)| (Reverse(*score), s.name.clone()));

    Some(
        scored
            .into_iter()
            .map(|(_, s)| Completion {
                value: format!("/skill:{}", s.name),
                description: Some(format!("{} [{}]", s.description, s.origin)),
                replacement: skill_idx..cursor,
            })
            .collect(),
    )
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
                value: format!("@{f}"),
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
    let mut scored: Vec<(i64, &CommandItem)> = commands
        .iter()
        .filter_map(|cmd| {
            let cmd_name = cmd.name.trim_start_matches('/');
            fuzzy_score(query, cmd_name).map(|score| (score, cmd))
        })
        .collect();

    scored.sort_by_key(|(score, cmd)| (Reverse(*score), cmd.name.clone()));

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
    if let Some(rest) = prefix.strip_prefix("/mcp ") {
        return Some(complete_mcp(&set.mcp_servers.snapshot(), rest, cursor));
    }
    complete_auth_args(set, prefix, cursor)
}

fn rank_by_name<'a, T>(items: &'a [T], query: &str, name: impl Fn(&T) -> &str) -> Vec<&'a T> {
    let mut scored: Vec<(i64, &T)> = items
        .iter()
        .filter_map(|item| {
            if query.is_empty() {
                Some((0, item))
            } else {
                fuzzy_score(query, name(item)).map(|score| (score, item))
            }
        })
        .collect();
    scored.sort_by(|(a_score, a), (b_score, b)| b_score.cmp(a_score).then_with(|| name(a).cmp(name(b))));
    scored.into_iter().map(|(_, item)| item).collect()
}

fn complete_mcp(servers: &[McpServerItem], rest: &str, cursor: usize) -> Vec<Completion> {
    let Some((sub, argument)) = rest.split_once(' ') else {
        return rank_by_name(MCP_SUBCOMMANDS, rest, |(name, _)| name)
            .into_iter()
            .map(|(name, desc)| Completion {
                value: format!("/mcp {name} "),
                description: Some((*desc).to_string()),
                replacement: 0..cursor,
            })
            .collect();
    };
    let oauth_only = match sub {
        "login" | "logout" => true,
        "prompts" | "prompt" => false,
        _ => return Vec::new(),
    };
    if argument.contains(' ') {
        return Vec::new();
    }
    let candidates: Vec<McpServerItem> = servers.iter().filter(|s| s.oauth || !oauth_only).cloned().collect();
    rank_by_name(&candidates, argument, |s| &s.name)
        .into_iter()
        .map(|s| Completion {
            value: format!("/mcp {sub} {}", s.name),
            description: Some(s.target.clone()),
            replacement: 0..cursor,
        })
        .collect()
}

fn complete_skills(skills: &[SkillItem], argument: &str, cursor: usize) -> Vec<Completion> {
    let mut scored: Vec<(i64, &SkillItem)> = skills
        .iter()
        .filter_map(|s| {
            if argument.is_empty() {
                Some((0, s))
            } else {
                fuzzy_score(argument, &s.name).map(|score| (score, s))
            }
        })
        .collect();
    scored.sort_by_key(|(score, s)| (Reverse(*score), s.name.clone()));

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
    let mut scored: Vec<(i64, &ModelItem)> = models
        .iter()
        .filter_map(|m| {
            if argument.is_empty() {
                Some((0, m))
            } else {
                let query_target = format!("{}:{}", m.provider, m.id);
                fuzzy_score(argument, &m.id)
                    .or_else(|| fuzzy_score(argument, &query_target))
                    .map(|score| (score, m))
            }
        })
        .collect();
    scored.sort_by_key(|(score, m)| (Reverse(*score), m.id.clone()));

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
    let mut scored: Vec<(i64, &ProviderItem)> = providers
        .iter()
        .filter_map(|p| {
            if target.argument.is_empty() {
                Some((0, p))
            } else {
                fuzzy_score(target.argument, &p.name).map(|score| (score, p))
            }
        })
        .collect();
    scored.sort_by_key(|(score, p)| (Reverse(*score), p.name.clone()));

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
    let mut scored: Vec<(i64, &(&str, &str))> = THINKING_LEVELS
        .iter()
        .filter_map(|lvl| {
            if argument.is_empty() {
                Some((0, lvl))
            } else {
                fuzzy_score(argument, lvl.0).map(|score| (score, lvl))
            }
        })
        .collect();
    scored.sort_by_key(|(score, lvl)| (Reverse(*score), lvl.0));

    scored
        .into_iter()
        .map(|(_, (level, desc))| Completion {
            value: format!("/thinking {level}"),
            description: Some((*desc).to_string()),
            replacement: 0..cursor,
        })
        .collect()
}
