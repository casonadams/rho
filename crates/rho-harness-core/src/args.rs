//! Host-side data shapes for built-in tool arguments.

use serde::{Deserialize, Serialize};

pub const DEFAULT_BASH_TIMEOUT_SEC: u64 = 30;
pub const DEFAULT_READ_LIMIT: usize = 2000;

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct BashArgs {
    /// Command to execute
    pub command: String,
    /// Timeout in seconds (default: 30). On timeout the command keeps running as a background job unless background is false
    #[serde(default)]
    pub timeout: Option<u64>,
    /// true: run in the background and return a job id immediately. false: kill the command on timeout. Omitted: wait, then move to the background on timeout
    #[serde(default)]
    pub background: Option<bool>,
}

impl BashArgs {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            timeout: None,
            background: None,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct EditReplacement {
    /// Exact text to replace; must match exactly once unless replaceAll is true. Omit when using startText/endText
    #[serde(rename = "oldText", default, skip_serializing_if = "String::is_empty")]
    pub old_text: String,
    /// Replacement text
    #[serde(rename = "newText")]
    pub new_text: String,
    /// Replace a whole region instead of oldText: exact text where the region starts (must match exactly once)
    #[serde(rename = "startText", default, skip_serializing_if = "Option::is_none")]
    pub start_text: Option<String>,
    /// Exact text where the region ends; the first match after startText closes the region (both anchors are replaced)
    #[serde(rename = "endText", default, skip_serializing_if = "Option::is_none")]
    pub end_text: Option<String>,
    /// Replace every occurrence of oldText instead of requiring a unique match (default: false)
    #[serde(rename = "replaceAll", default, skip_serializing_if = "Option::is_none")]
    pub replace_all: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct EditArgs {
    /// Path to the file to edit (relative or absolute)
    pub path: String,
    /// List of exact replacements to apply
    pub edits: Vec<EditReplacement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FdSort {
    #[default]
    Path,
    Lines,
    Size,
}

#[derive(Debug, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct FdArgs {
    /// Smart-case regex matched unanchored against each entry's workspace-relative path (case-insensitive unless it contains an uppercase character). If omitted, matches all entries.
    pub pattern: Option<String>,
    /// Subdirectory to search, relative to the workspace root (default: workspace root)
    pub path: Option<String>,
    /// Filter entries by type using default definitions (e.g. 'rust', 'py'); unknown names are rejected
    #[serde(rename = "type")]
    pub file_type: Option<String>,
    /// Include hidden dotfiles; paths excluded by ignore rules (.gitignore, .ignore) stay excluded
    pub hidden: Option<bool>,
    /// Maximum traversal depth, clamped to 1-10 when provided (default: unlimited)
    pub depth: Option<usize>,
    /// Number of results to skip, for paging (default: 0); truncated output names the next offset
    pub offset: Option<usize>,
    /// Maximum number of results per page (default: as many as fit in 20KB)
    pub limit: Option<usize>,
    /// Include line count and byte size in output (default: false; enabled automatically if min_lines, max_lines, or sort is set)
    pub stats: Option<bool>,
    /// Minimum line count filter (e.g. 150 to identify oversized files)
    pub min_lines: Option<usize>,
    /// Maximum line count filter
    pub max_lines: Option<usize>,
    /// Sort order: 'path' (ascending, default), 'lines' (descending), or 'size' (descending)
    pub sort: Option<FdSort>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ReadArgs {
    /// Path to the file to read (relative or absolute)
    pub path: String,
    /// Line number to start reading from (1-indexed, default: 1)
    pub offset: Option<usize>,
    /// Maximum number of lines to read (default: 2000)
    pub limit: Option<usize>,
    /// Read only this symbol's lines, found by the file's language server (e.g. 'parse', 'Config.load', 'Config::load'); offset and limit are ignored
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct RgArgs {
    /// Smart-case regex matched line-by-line against file contents (case-insensitive unless it contains an uppercase character)
    pub pattern: String,
    /// Subdirectory or file to search, relative to the workspace root (default: workspace root)
    pub path: Option<String>,
    /// Filter files by type using default definitions (e.g. 'rust', 'py'); unknown names are rejected
    #[serde(rename = "type")]
    pub file_type: Option<String>,
    /// Include hidden dotfiles; paths excluded by ignore rules (.gitignore, .ignore) stay excluded
    pub hidden: Option<bool>,
    /// 'content' (matching lines grouped by file, default), 'files' (matching file paths only), or 'count' (match count per file)
    pub output_mode: Option<RgOutputMode>,
    /// Lines of context to show before and after each match in 'content' mode (0-10, default: 0); context lines use `LINE-text`, and `--` separates non-adjacent blocks
    pub context: Option<usize>,
    /// Let the pattern match across lines (use `\n` in the pattern); each multi-line match is shown as consecutive `LINE:text` rows
    pub multiline: Option<bool>,
    /// Number of results to skip, for paging (default: 0); truncated output names the next offset
    pub offset: Option<usize>,
    /// Maximum number of results per page (default: as many as fit in 20KB)
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LspAction {
    /// Errors and warnings for `path` (synced from disk), or every file servers have reported without `path`
    Diagnostics,
    /// Where the symbol at `path`/`line`/`symbol` is defined
    Definition,
    /// Every use of the symbol at `path`/`line`/`symbol`
    References,
    /// Type and documentation of the symbol at `path`/`line`/`symbol`
    Hover,
    /// Implementations of the trait, interface, or method at `path`/`line`/`symbol`
    Implementation,
    /// Where the type of the symbol at `path`/`line`/`symbol` is defined
    TypeDefinition,
    /// Outline of `path` with line ranges, or with `query` and no `path`, a workspace-wide symbol search
    Symbols,
    /// Rename the symbol at `path`/`line`/`symbol` to `new_name` across the workspace and write the files
    Rename,
    /// Configured servers and which are running
    Status,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct LspArgs {
    /// What to do
    pub action: LspAction,
    /// File to query (relative or absolute)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// 1-based line of the symbol, as shown by read or rg
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    /// Text of the symbol on that line (e.g. 'parse_args'); its first whole-word occurrence is used. Default: the first non-blank character
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Symbol name to search for across the workspace (symbols action)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// New name for rename
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RgOutputMode {
    #[default]
    Content,
    Files,
    Count,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct WebFetchArgs {
    /// URL to fetch
    pub url: String,
    /// Line number to start reading from (1-indexed, default 1)
    pub offset: Option<usize>,
    /// Maximum number of lines to return (default 200 unless configured)
    pub limit: Option<usize>,
    /// Extraction mode: "auto" (main content, falling back to the full page; default), "main" (main content or error), or "full"
    pub mode: Option<String>,
    /// Optional format override when detection is wrong ("html", "json", "markdown", "csv", "tsv", "xml", "rss", "atom", "pdf")
    pub format: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WebSearchRecency {
    Day,
    Week,
    Month,
    Year,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct WebSearchArgs {
    /// Search query
    pub query: String,
    /// Maximum number of search results to return (default: 5, max: 20)
    pub limit: Option<usize>,
    /// Filter search results by time period: 'day', 'week', 'month', or 'year'
    pub recency: Option<WebSearchRecency>,
    /// Limit results to specific domains (e.g. ['github.com']) or exclude domains with a leading '-' (e.g. ['-spam.com'])
    pub domains: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct WriteArgs {
    /// Path to the file to write (relative or absolute)
    pub path: String,
    /// Content to write to the file
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubagentRole {
    #[default]
    Scout,
    Critic,
    Planner,
    General,
}

impl SubagentRole {
    pub const ALL: [Self; 4] = [Self::Scout, Self::Critic, Self::Planner, Self::General];

    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Scout => "scout",
            Self::Critic => "critic",
            Self::Planner => "planner",
            Self::General => "general",
        }
    }

    #[must_use]
    pub fn allowed_tools(&self) -> &'static [&'static str] {
        match self {
            Self::Scout => &["read", "fd", "rg", "web_fetch", "web_search"],
            Self::Critic => &["read", "fd", "rg"],
            Self::Planner => &["read", "fd", "rg"],
            Self::General => &["read", "write", "edit", "bash", "fd", "rg", "web_fetch", "web_search"],
        }
    }

    #[must_use]
    pub fn system_instructions(&self) -> &'static str {
        match self {
            Self::Scout => {
                "You are an exploratory scout. Your role is fast, read-only inspection of the repository. Use fd, rg, and read to find files, locate symbols, and understand code structure. Plan targeted queries and synthesize your factual summary before exhausting turns. Do not speculate."
            }
            Self::Critic => {
                "You are an adversarial reviewer. Your role is strictly to find bugs, edge cases, race conditions, missing test assertions, security flaws, and violated invariants. Be rigorous and objective. Point out specific lines and failure modes."
            }
            Self::Planner => {
                "You are a software architect and planner. Your role is to decompose tasks into clean, sequential, testable vertical slices and identify risks, invariants, and reuse opportunities. Do not write full code solutions."
            }
            Self::General => {
                "You are a focused problem solver assisting the primary agent. Execute the assigned task efficiently and provide a clear, concise summary of the outcome."
            }
        }
    }
}

impl std::str::FromStr for SubagentRole {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let norm = s.trim().to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|role| role.as_str() == norm)
            .ok_or_else(|| format!("unknown subagent role: '{s}'"))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct SubagentArgs {
    /// Continue an earlier subagent task by the id its result reported: answer its question or send review feedback (e.g. 'you missed X, fix it'). It keeps its full history; role, agent, persona, and name come from that task. While the task still runs, the prompt is queued as a message it reads before its next model call
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// Short camelCase name for what this subagent is doing (e.g. 'performanceAudit'); labels its progress, /agents entry, and result
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Role profile for the subagent ('scout', 'critic', 'planner', 'general'; default: scout). Only 'general' can write, edit, or run bash
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<SubagentRole>,
    /// Custom subagent name to invoke (e.g. defined in .claude/agents/ or .agents/agents/)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Persona whose instructions are layered onto the role or agent (see the personas listed in this tool's description); tools still come from the role or agent
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona: Option<String>,
    /// Read-only: with `task_id`, show that task's messages, final answer, every tool call and its outcome, and the files it changed; without, list the session's subagent tasks. Runs no model and needs no prompt
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inspect: Option<bool>,
    /// Task prompt or instructions for the subagent to execute (required unless `inspect` is true)
    #[serde(default)]
    pub prompt: String,
    /// Optional context slice or snippet to prime the subagent
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_slice: Option<String>,
    /// Model role from settings: 'default', 'smol', or 'slow' (default: the configured model for the persona, role, or agent, else smol when configured, otherwise the session model)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Maximum model calls for the subagent (default: 100)
    #[serde(default, alias = "max_turns", skip_serializing_if = "Option::is_none")]
    pub turns: Option<usize>,
    /// Share the recent parent conversation (text only) with the subagent; the subagent's model provider will see it
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherit_context: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum WorktreeAction {
    /// List every agent worktree in this repository, oldest first
    List,
    /// Show a worktree's changes as a stat summary and patch
    Diff,
    /// Three-way merge a worktree's changes into your working tree, leaving conflict markers to fix, then remove it
    Merge,
    /// Delete a worktree and its changes
    Discard,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct WorktreeArgs {
    /// What to do
    pub action: WorktreeAction,
    /// Worktree id from a subagent result or workflow report (required except for list)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

// Submodule aliases for backwards compatibility
pub mod bash {
    pub use super::{BashArgs, DEFAULT_BASH_TIMEOUT_SEC};
}
pub mod edit {
    pub use super::{EditArgs, EditReplacement};
}
pub mod fd {
    pub use super::{FdArgs, FdSort};
}
pub mod read {
    pub use super::{DEFAULT_READ_LIMIT, ReadArgs};
}
pub mod lsp {
    pub use super::{LspAction, LspArgs};
}
pub mod rg {
    pub use super::{RgArgs, RgOutputMode};
}
pub mod web_fetch {
    pub use super::WebFetchArgs;
}
pub mod web_search {
    pub use super::{WebSearchArgs, WebSearchRecency};
}
pub mod write {
    pub use super::WriteArgs;
}
pub mod subagent {
    pub use super::{SubagentArgs, SubagentRole};
}
pub mod worktree {
    pub use super::{WorktreeAction, WorktreeArgs};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subagent_role_properties() {
        assert_eq!(SubagentRole::default(), SubagentRole::Scout);
        assert_eq!(SubagentRole::Scout.as_str(), "scout");

        assert_eq!(
            SubagentRole::Scout.allowed_tools(),
            &["read", "fd", "rg", "web_fetch", "web_search"]
        );
        assert_eq!(SubagentRole::Critic.allowed_tools(), &["read", "fd", "rg"]);
        assert_eq!(SubagentRole::Planner.allowed_tools(), &["read", "fd", "rg"]);
    }

    #[test]
    fn test_subagent_args_serde() {
        let json = r#"{"role":"scout","prompt":"find main.rs","max_turns":5,"model":"slow"}"#;
        let parsed: SubagentArgs = serde_json::from_str(json).expect("valid json");
        assert_eq!(parsed.role, Some(SubagentRole::Scout));
        assert_eq!(parsed.prompt, "find main.rs");
        assert_eq!(parsed.turns, Some(5));
        assert_eq!(parsed.model.as_deref(), Some("slow"));

        let serialized = serde_json::to_string(&parsed).expect("can serialize");
        assert!(serialized.contains(r#""role":"scout""#));
    }
}
