use crate::tools::bash::BashArgs;
use crate::tools::edit::EditArgs;
use crate::tools::fd::FdArgs;
use crate::tools::read::ReadArgs;
use crate::tools::rg::RgArgs;
use crate::tools::types::generated_schema;
use crate::tools::web::fetch::WebFetchArgs;
use crate::tools::web::search::WebSearchArgs;
use crate::tools::write::WriteArgs;
use rho_harness_core::args::{SubagentArgs, WorktreeArgs};

pub static PROMPT_WORKTREE: &str = "\
Review and land work that subagents and workflows did in their own git worktrees.

Usage:
- Every subagent and workflow works in its own worktree, a separate checkout outside your working tree. Their changes reach your working tree only when you merge them.
- `list` shows worktrees with unmerged changes; `diff` shows one worktree's changes.
- Review before merging: read the diff, and run checks in the worktree directory if needed.
- `merge` applies the changes to your working tree with a three-way merge against your current files (uncommitted edits included), then removes the worktree. Files changed on both sides get <<<<<<< conflict markers: resolve them with edit, then re-run checks.
- `discard` deletes a worktree you do not want.
- You own the worktrees your subagents and workflows create: merge or discard each before you finish, or tell the user why one is kept. Unmerged worktrees are deleted 24 hours after they were created.
- To have a subagent fix its work before merging, continue its task with the subagent tool's `task_id`; it keeps working in the same worktree.";

pub static PROMPT_READ: &str = "\
Read file contents with offset and limit safeguards.

Usage:
- Use read to examine files instead of cat or sed.
- Use offset and limit when reading large files.
- Truncates lines when output exceeds maximum byte bounds.
- Supported images (png, jpeg, gif, webp, bmp) are attached to the result instead of being returned as text.";

pub static PROMPT_WRITE: &str = "\
Create or overwrite files. Automatically creates parent directories.

Usage:
- Use write only for new files or complete rewrites.
- For small or targeted changes to existing files, prefer edit instead.
- Use write instead of shell redirects, heredocs, or scripts to create files.";

pub static PROMPT_EDIT: &str = "\
Edit a file with exact text replacements, applied in order.

Usage:
- Each edits[] entry is one of:
  - {oldText, newText}: oldText must match exactly once; keep it as small as possible while still unique.
  - {oldText, newText, replaceAll: true}: replace every occurrence, e.g. a rename within the file.
  - {startText, endText, newText}: replace the region from startText through the first endText after it, inclusive; use this to delete or rewrite a block without copying it into oldText.
- Put all changes to one file in one call; for changes across files, make one edit call per file in the same response.
- If two changes touch the same lines, merge them into one entry instead of emitting overlapping edits.
- Use edit for every change to an existing file; never sed -i, perl -pi, python or awk scripts, or shell redirects.";

pub static PROMPT_FD: &str = "\
Find files and directories by workspace-relative path pattern.

Usage:
- pattern is an optional smart-case regex (case-insensitive unless it contains an uppercase character) matched against workspace-relative paths; if omitted, all entries in the search root match.
- Files and directories both match; results are sorted lexicographically by default and capped at limit (default 200, max 1000) with a 20,000-entry collection ceiling; output is byte-capped at 50KB.
- Ignore rules (.gitignore, .ignore) and hidden entries are respected by default; set hidden: true to include hidden dotfiles.
- Use type (e.g. 'rust', 'py') to filter by file type and depth (1-10) for a bounded overview, e.g. depth 2 for top-level layout.
- Set stats: true to include line counts and byte sizes; use min_lines or max_lines to filter by line count (e.g. min_lines: 150 to identify oversized files), and sort ('lines', 'size', or 'path') to order results.";

pub static PROMPT_RG: &str = "\
Search file contents with line-oriented results.

Usage:
- pattern is a smart-case regex (case-insensitive unless it contains an uppercase character) matched against file contents.
- Returns path:line: text lines sorted by path then line number; match lines are truncated at 500 characters.
- Results are capped at limit (default 200, max 1000) with a 5,000-match collection ceiling; output is byte-capped at 50KB.
- Ignore rules (.gitignore, .ignore) and hidden entries are respected by default; set hidden: true to include hidden dotfiles.
- Binary files and files over 1 MB are skipped; use type (e.g. 'rust', 'py') to filter by file type.";

pub static PROMPT_BASH: &str = "\
Execute bash commands in the current working directory.

Usage:
- Commands run directly in the working directory; do not prefix commands with cd.
- Use fd for file discovery, rg for content search; use bash for git actions, cargo builds, tests, and linters.
- Never read files with cat, sed, or awk, and never create or modify files with bash (sed -i, perl -pi, python or awk scripts, echo or cat redirects, tee, mv over a file); use read, edit, and write. Project formatters and generators (cargo fmt, make) are fine.
- Captures combined stdout and stderr with output truncation safeguards.";

pub static PROMPT_WEB_SEARCH: &str = "\
Search the web and return structured summaries and URLs.

Usage:
- Prefer web_search for finding public documentation, repositories, package releases, and technical references.
- Use recency ('day', 'week', 'month', 'year') to filter results by freshness.
- Use domains to limit results to specific domains (e.g. ['github.com']) or exclude domains with a leading '-' (e.g. ['-spam.com']).
- Returns concise result summaries with title, URL, and snippet.";

pub static PROMPT_WEB_FETCH: &str = "\
Fetch HTML, JSON, Markdown, text, or PDF content from a URL and return clean text.

Usage:
- Extracts clean markdown/text without navigation bloat or HTML tags.
- Use mode: 'full' when navigation or sidebars are needed.
- Respects byte limits, caching, and rate limiting safeguards.";

pub static PROMPT_SUBAGENT: &str = "\
Spawn a specialized subagent in an isolated context to perform focused research, code review, or planning.

Usage:
- Use role 'scout' for read-only codebase exploration, searching files, and locating symbols without polluting parent context.
- Use role 'critic' for adversarial code review, bug hunting, and invariant verification.
- Use role 'planner' to draft execution steps or trade-off analyses without mutating files.
- Use role 'general' for self-contained multi-step tasks.
- Subagents run up to `turns` model calls (default 100) on the `model` role you pass ('default', 'smol', or 'slow'; default smol) and return a concise summary to the parent.
- Always pass `name`: a short camelCase label for the task (e.g. 'performanceAudit', 'retryAfterReview'); it identifies the subagent in progress lines, /agents, its task id, and its result.
- Each result ends with a task id and state. Treat the answer as a claim, not proof: before accepting, re-read the files it changed and re-run the relevant checks yourself. If something was missed or wrong, call subagent again with that `task_id` and your feedback (e.g. 'you missed X, fix it'); the subagent continues with everything it already read and did. Repeat until the work is right.
- Results list 'Files changed' (its write/edit calls), 'Also changed in the workspace' (bash, formatters, other agents), and 'Tool outcomes' (errors and permission denials). A footer with 'failed tool calls' means part of the work may not have happened even if the answer says it did.
- Pass `inspect: true` without `task_id` to list the session's subagent tasks (use it to recover task ids after compaction); with a `task_id` it shows that task's messages, final answer, every tool call with its outcome, and its files, without running it.
- State 'input-required' means the subagent is waiting: it asked you a question ('Question for you: ...') or stopped early (turn limit or no final answer) with a summary. Reply with the same `task_id` and your answer or 'continue'. Do not redo its work yourself.
- State 'failed' still reports any findings and 'Files changed'; build on them or send the `task_id` follow-up instead of starting over.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinToolKind {
    ReadOnly,
    WorkspaceMutation,
    Network,
    Shell,
    Composite,
}

#[derive(Debug, Clone, Copy)]
pub struct BuiltinToolDeclaration {
    pub name: &'static str,
    pub capability: BuiltinToolKind,
    pub prompt: &'static str,
    pub prompt_snippet: &'static str,
    pub(crate) schema: fn() -> serde_json::Value,
}

impl BuiltinToolDeclaration {
    pub fn schema(&self) -> serde_json::Value {
        (self.schema)()
    }
}

pub const DECLARATIONS: &[BuiltinToolDeclaration] = &[
    BuiltinToolDeclaration {
        name: "read",
        capability: BuiltinToolKind::ReadOnly,
        prompt: PROMPT_READ,
        prompt_snippet: "Read file contents (with line numbering, offset, and limit safeguards)",
        schema: generated_schema::<ReadArgs>,
    },
    BuiltinToolDeclaration {
        name: "write",
        capability: BuiltinToolKind::WorkspaceMutation,
        prompt: PROMPT_WRITE,
        prompt_snippet: "Create or overwrite files (automatically creates parent directories)",
        schema: generated_schema::<WriteArgs>,
    },
    BuiltinToolDeclaration {
        name: "edit",
        capability: BuiltinToolKind::WorkspaceMutation,
        prompt: PROMPT_EDIT,
        prompt_snippet: "Change existing files with exact text replacements, replaceAll, or startText/endText regions",
        schema: generated_schema::<EditArgs>,
    },
    BuiltinToolDeclaration {
        name: "bash",
        capability: BuiltinToolKind::Shell,
        prompt: PROMPT_BASH,
        prompt_snippet: "Run git, builds, tests, and linters in the current working directory",
        schema: generated_schema::<BashArgs>,
    },
    BuiltinToolDeclaration {
        name: "fd",
        capability: BuiltinToolKind::ReadOnly,
        prompt: PROMPT_FD,
        prompt_snippet: "Find files and directories by workspace-relative path pattern (gitignore-aware, bounded)",
        schema: generated_schema::<FdArgs>,
    },
    BuiltinToolDeclaration {
        name: "rg",
        capability: BuiltinToolKind::ReadOnly,
        prompt: PROMPT_RG,
        prompt_snippet: "Search file contents with line-oriented results (gitignore-aware, skips binary and large files, bounded)",
        schema: generated_schema::<RgArgs>,
    },
    BuiltinToolDeclaration {
        name: "web_search",
        capability: BuiltinToolKind::Network,
        prompt: PROMPT_WEB_SEARCH,
        prompt_snippet: "Search the web and return structured summaries and URLs",
        schema: generated_schema::<WebSearchArgs>,
    },
    BuiltinToolDeclaration {
        name: "web_fetch",
        capability: BuiltinToolKind::Network,
        prompt: PROMPT_WEB_FETCH,
        prompt_snippet: "Fetch and extract clean text or markdown from URLs",
        schema: generated_schema::<WebFetchArgs>,
    },
    BuiltinToolDeclaration {
        name: "subagent",
        capability: BuiltinToolKind::Composite,
        prompt: PROMPT_SUBAGENT,
        prompt_snippet: "Spawn an isolated subagent with a specialized role ('scout', 'critic', 'planner', 'general')",
        schema: generated_schema::<SubagentArgs>,
    },
    BuiltinToolDeclaration {
        name: "worktree",
        capability: BuiltinToolKind::WorkspaceMutation,
        prompt: PROMPT_WORKTREE,
        prompt_snippet: "Review, merge, or discard the git worktrees where subagents and workflows did their work",
        schema: generated_schema::<WorktreeArgs>,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subagent_declaration_valid() {
        let sub = DECLARATIONS
            .iter()
            .find(|d| d.name == "subagent")
            .expect("subagent declaration must exist");
        assert_eq!(sub.capability, BuiltinToolKind::Composite);
        let sub_props = sub.schema()["properties"].clone();
        assert!(sub_props.get("prompt").is_some());
        assert!(sub_props.get("role").is_some());
    }
}
