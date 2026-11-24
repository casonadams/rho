/// Controls whether a slash command may execute while an agent turn is in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidTurnPolicy {
    /// Execute immediately without interrupting the active turn (UI modals, read-only info, non-mutating settings).
    Immediate,
    /// Queue as a follow-up until the active turn completes (commands that mutate conversation or engine state).
    Queue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubcommandSpec {
    pub usage: &'static str,
    pub summary: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShortcutSpec {
    pub key: &'static str,
    pub summary: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinCommand {
    Help,
    Settings,
    Models,
    Model,
    Thinking,
    Tools,
    Agents,
    Resume,
    Skill,
    Mcp,
    Session,
    Compact,
    Tree,
    Fork,
    Clone,
    Name,
    Rewind,
    Clear,
    Login,
    Logout,
    Reload,
    Export,
    Workflow,
    Worktree,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlashCommandSpec {
    pub id: BuiltinCommand,
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub usage: &'static str,
    pub summary: &'static str,
    pub detail: &'static str,
    pub subcommands: &'static [SubcommandSpec],
    pub mid_turn: MidTurnPolicy,
}

const MCP_SUBCOMMANDS: &[SubcommandSpec] = &[
    SubcommandSpec {
        usage: "/mcp login <server>",
        summary: "Sign in to an OAuth MCP server",
    },
    SubcommandSpec {
        usage: "/mcp logout <server>",
        summary: "Remove stored OAuth credentials for an MCP server",
    },
    SubcommandSpec {
        usage: "/mcp prompts [server]",
        summary: "List MCP server prompts",
    },
    SubcommandSpec {
        usage: "/mcp prompt <server> <name> [key=value ...]",
        summary: "Send an MCP prompt with optional key=value arguments",
    },
];

pub const BUILTIN_COMMANDS: &[SlashCommandSpec] = &[
    SlashCommandSpec {
        id: BuiltinCommand::Help,
        name: "help",
        aliases: &[],
        usage: "/help [command]",
        summary: "Show command reference or extended help for a command",
        detail: "Opens the interactive help modal when run with no arguments in TUI mode, or prints the full command and shortcut reference. Pass a command name (e.g. /help mcp or /help wt) for detailed usage and subcommands.",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Immediate,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Settings,
        name: "settings",
        aliases: &[],
        usage: "/settings",
        summary: "Interactive runtime interface settings",
        detail: "Opens the in-TUI settings modal to adjust block style, response boxing, thinking effort, thinking visibility, tool output expansion, version banner, cursor style, model roles, and tools & permissions. Works mid-turn.",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Immediate,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Models,
        name: "models",
        aliases: &[],
        usage: "/models",
        summary: "Configure role models (default, smol, slow, guard)",
        detail: "Opens the role-model configuration modal to assign models for default, smol, slow, and guard roles. Changes persist to config.toml and work mid-turn.",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Immediate,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Model,
        name: "model",
        aliases: &[],
        usage: "/model [<provider>/<model>]",
        summary: "Inspect or switch the active model",
        detail: "Without arguments, opens the interactive model selector (Ctrl+L). Pass <model> or <provider>/<model> to switch immediately. Safe to invoke mid-turn (applied to the next turn).",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Immediate,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Thinking,
        name: "thinking",
        aliases: &["think"],
        usage: "/thinking [off|low|medium|high|max]",
        summary: "Inspect or set the reasoning / thinking effort level",
        detail: "Sets the thinking effort level for the session (off, minimal, low, medium, high, max). Without arguments in TUI mode, opens Settings. Safe to invoke mid-turn.",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Immediate,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Tools,
        name: "tools",
        aliases: &["permissions"],
        usage: "/tools",
        summary: "Configure built-in tools, search engine, MCP, and permissions",
        detail: "Opens the Tools & Permissions modal to toggle web search, web fetch, default search engine, MCP, and permission checks. Changes persist to config.toml and work mid-turn.",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Immediate,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Agents,
        name: "agents",
        aliases: &[],
        usage: "/agents",
        summary: "Inspect and interact with running or completed subagents",
        detail: "Opens the subagent monitor modal (same as pressing Left on an empty prompt) to inspect, focus, steer, or stop subagents. Works mid-turn.",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Immediate,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Resume,
        name: "resume",
        aliases: &[],
        usage: "/resume [session_id]",
        summary: "Resume a prior session",
        detail: "Without arguments in TUI mode, opens the interactive session picker. Pass a session ID to switch directly to that session.",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Queue,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Skill,
        name: "skill",
        aliases: &["skills"],
        usage: "/skill [name]",
        summary: "List or inspect declarative skills",
        detail: "Lists discovered skills when run without arguments, or prints the metadata and location of a specific skill. Invoke skills inline with /skill:<name>.",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Queue,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Mcp,
        name: "mcp",
        aliases: &[],
        usage: "/mcp [login|logout|prompts|prompt ...]",
        summary: "Inspect and manage Model Context Protocol servers",
        detail: "Lists configured MCP servers (or opens the MCP selector in TUI mode). Use subcommands to authenticate OAuth servers or list and invoke MCP prompts.",
        subcommands: MCP_SUBCOMMANDS,
        mid_turn: MidTurnPolicy::Queue,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Session,
        name: "session",
        aliases: &["tokens"],
        usage: "/session",
        summary: "Display token capacity and session diagnostics",
        detail: "Prints active session ID, model, provider, thinking level, context window usage, prompt cache hit rate, token totals, and generation speed. Works mid-turn.",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Immediate,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Compact,
        name: "compact",
        aliases: &[],
        usage: "/compact [instructions]",
        summary: "Summarize earlier context to free context space",
        detail: "Compacts older conversation history into a summary while preserving recent turns. Optional instructions guide what the summary should emphasize.",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Queue,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Tree,
        name: "tree",
        aliases: &[],
        usage: "/tree",
        summary: "View conversation turn and branch tree",
        detail: "Opens the session tree navigator to inspect branches, jump to earlier checkpoints, or label nodes.",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Queue,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Fork,
        name: "fork",
        aliases: &[],
        usage: "/fork [turn|node_id]",
        summary: "Fork session from a turn or node into a new session",
        detail: "Creates a new session branched from the specified turn number or node ID (or the current leaf if omitted) and switches to it.",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Queue,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Clone,
        name: "clone",
        aliases: &[],
        usage: "/clone",
        summary: "Duplicate active branch into a new session",
        detail: "Duplicates the current session at the active branch leaf into a new independent session.",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Queue,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Name,
        name: "name",
        aliases: &[],
        usage: "/name <session_name>",
        summary: "Assign a human-readable name to the session",
        detail: "Sets the display name for the current session shown in /resume and session metadata.",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Queue,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Rewind,
        name: "rewind",
        aliases: &[],
        usage: "/rewind <turn>",
        summary: "Rewind context to a specific prior turn",
        detail: "Moves the active branch pointer back to the given 1-based turn number without deleting history.",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Queue,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Clear,
        name: "clear",
        aliases: &["new", "reset"],
        usage: "/clear",
        summary: "Start a new session while preserving history",
        detail: "Starts a fresh session with empty context while keeping previous sessions saved on disk.",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Queue,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Login,
        name: "login",
        aliases: &[],
        usage: "/login [provider]",
        summary: "Add API-key or subscription auth",
        detail: "Opens the provider login selector or initiates authentication for the specified provider or mcp:<server>.",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Queue,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Logout,
        name: "logout",
        aliases: &[],
        usage: "/logout [provider]",
        summary: "Remove stored provider auth",
        detail: "Removes stored credentials for the specified provider (or prompts to choose one).",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Queue,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Reload,
        name: "reload",
        aliases: &[],
        usage: "/reload",
        summary: "Re-read config, skills, and MCP tools while keeping history",
        detail: "Reloads configuration files, prompt templates, skills, and MCP server connections without clearing conversation history.",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Queue,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Export,
        name: "export",
        aliases: &[],
        usage: "/export [html|md] [path]",
        summary: "Export the active branch as a readable artifact",
        detail: "Exports the active conversation branch as HTML (default) or Markdown to a default filename or explicit output path.",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Queue,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Workflow,
        name: "workflow",
        aliases: &[],
        usage: "/workflow [name] [inputs]",
        summary: "List workflows, or run one (Left watches agents)",
        detail: "Lists discovered workflows when run without arguments, or starts/resumes a workflow with key=value inputs.",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Queue,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Worktree,
        name: "wt",
        aliases: &["worktree"],
        usage: "/wt [branch]",
        summary: "Create a git worktree and move this session into it",
        detail: "Creates a new linked git worktree (using CoW reflink cloning where supported), carries over uncommitted changes, and relocates the live session into it.",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Queue,
    },
    SlashCommandSpec {
        id: BuiltinCommand::Exit,
        name: "exit",
        aliases: &["quit"],
        usage: "/exit",
        summary: "Exit rho",
        detail: "Cleanly exits the interactive session and prints a resume command for the current session.",
        subcommands: &[],
        mid_turn: MidTurnPolicy::Queue,
    },
];

pub const SHORTCUTS: &[ShortcutSpec] = &[
    ShortcutSpec {
        key: "Tab",
        summary: "Complete slash commands & skill names",
    },
    ShortcutSpec {
        key: "Shift+Tab",
        summary: "Cycle thinking level",
    },
    ShortcutSpec {
        key: "Ctrl+L",
        summary: "Select model",
    },
    ShortcutSpec {
        key: "Ctrl+O",
        summary: "Expand or collapse tool output",
    },
    ShortcutSpec {
        key: "Ctrl+T",
        summary: "Toggle thinking blocks visibility",
    },
    ShortcutSpec {
        key: "Ctrl+C",
        summary: "Clear the input prompt",
    },
    ShortcutSpec {
        key: "Ctrl+D",
        summary: "Exit at an empty prompt",
    },
    ShortcutSpec {
        key: "Left",
        summary: "Watch subagents (at an empty prompt)",
    },
    ShortcutSpec {
        key: "Escape",
        summary: "Cancel active execution or dismiss modal",
    },
];

/// Looks up a built-in command by primary name or alias (with or without leading `/`).
pub fn lookup_builtin(name: &str) -> Option<&'static SlashCommandSpec> {
    let query = name.trim().trim_start_matches('/');
    if query.is_empty() {
        return None;
    }
    BUILTIN_COMMANDS.iter().find(|spec| {
        spec.name.eq_ignore_ascii_case(query) || spec.aliases.iter().any(|alias| alias.eq_ignore_ascii_case(query))
    })
}

/// Returns the mid-turn execution policy for a slash input line.
pub fn mid_turn_policy(input: &str) -> MidTurnPolicy {
    let trimmed = input.trim();
    if !super::is_slash_command(trimmed) {
        return MidTurnPolicy::Queue;
    }
    let Some(first) = trimmed[1..].split_whitespace().next() else {
        return MidTurnPolicy::Queue;
    };
    lookup_builtin(first).map_or(MidTurnPolicy::Queue, |spec| spec.mid_turn)
}
