use crossterm::event::KeyEvent;

use super::super::ModalKeyResult;
use crate::error::Result;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};

const HELP_ENTRIES: &[(&str, &str)] = &[
    ("/settings", "Interactive runtime interface settings"),
    ("/models", "Configure role models (default, smol, slow, guard)"),
    ("/model", "Inspect or switch the active model"),
    ("/resume", "Resume a prior session"),
    ("/session", "Token capacity & diagnostics (alias: /tokens)"),
    ("/compact", "Summarize earlier context to free space"),
    ("/tree", "View conversation turn and branch tree"),
    ("/fork", "Fork session from turn into a new session"),
    ("/clone", "Duplicate active branch into a new session"),
    ("/name", "Assign a human-readable name to session"),
    ("/rewind", "Rewind context to a specific prior turn"),
    ("/clear", "Start a new session; preserve history (alias: /new)"),
    ("/mcp", "List configured MCP servers"),
    ("/mcp login <server>", "Sign in to an OAuth MCP server"),
    ("/mcp prompts [server]", "List MCP server prompts"),
    (
        "/mcp prompt <server> <name>",
        "Send an MCP prompt (key=value arguments)",
    ),
    ("/login", "Add API-key or subscription auth"),
    ("/logout", "Remove stored provider auth"),
    ("/skill", "List or inspect skills"),
    ("/reload", "Re-read config, skills, and MCP tools"),
    ("/export", "Export active branch as HTML or Markdown"),
    ("/workflow", "List workflows, or run one: /workflow <name> [inputs]"),
    (
        "/wt",
        "Create a git worktree and move this session into it (alias: /worktree)",
    ),
    ("/exit", "Exit rho (alias: /quit)"),
    ("Tab", "Complete slash commands & skill names"),
    ("Shift+Tab", "Cycle thinking level"),
    ("Ctrl+L", "Select model"),
    ("Ctrl+O", "Expand or collapse tool output"),
    ("Ctrl+T", "Toggle thinking blocks visibility"),
    ("Ctrl+C", "Clear the input prompt"),
    ("Ctrl+D", "Exit at an empty prompt"),
    ("Left", "Watch subagents (at an empty prompt)"),
    ("Escape", "Cancel active execution or dismiss modal"),
];

pub fn open_help_selector<B: TerminalBackend>(controller: &mut TerminalController<B>) {
    let options = HELP_ENTRIES
        .iter()
        .map(|(key, desc)| ModalOption::new(format!("{key:<15}"), Some(*desc)))
        .collect();
    let modal = ModalState::new("Help", "", options).with_search(true);
    controller.state_mut().push_modal(modal);
}

pub fn handle_help_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    crate::repl::live::modal::dispatch_simple_selector(controller, key, |opt| {
        let raw = opt.label.trim();
        raw.starts_with('/').then(|| ModalKeyResult::HelpCommandSelected {
            command: raw.to_string(),
        })
    })
}
