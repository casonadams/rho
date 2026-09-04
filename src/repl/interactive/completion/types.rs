use std::ops::Range;

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
