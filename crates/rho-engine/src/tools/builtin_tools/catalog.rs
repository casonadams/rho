use crate::tools::bash::BashArgs;
use crate::tools::edit::EditArgs;
use crate::tools::fd::FdArgs;
use crate::tools::lsp::LspArgs;
use crate::tools::read::ReadArgs;
use crate::tools::rg::RgArgs;
use crate::tools::types::generated_schema;
use crate::tools::web::fetch::WebFetchArgs;
use crate::tools::web::search::WebSearchArgs;
use crate::tools::write::WriteArgs;
use rho_harness_core::args::{MemoryArgs, SubagentArgs, TodoArgs, UndoArgs, WaitArgs, WorktreeArgs};

pub static PROMPT_WAIT: &str = "\
Block until one of your background bash jobs or subagent tasks finishes, or a user message arrives.

Usage:
- Use it only when you cannot continue without the result; otherwise keep working or end your turn, and results arrive on their own.
- It returns every result that has arrived, each only once; call it again to wait for work still running.
- It returns at once when nothing is running or a message is waiting, and after 10 minutes returns `Still running:` with what is left.";

pub static PROMPT_UNDO: &str = "\
Revert the most recent edit, write, or lsp change (rename, code action, format) this session made to a file.

Usage:
- Call again to step further back, up to 20 changes per file (fewer after very large edits). A file that write created is deleted. Bash commands are not undone.
- It refuses when the file changed since (by the user, bash, or a formatter); then read and edit it instead.
- The result shows the restored regions numbered like read.";

pub static PROMPT_MEMORY: &str = "\
Keep short notes that are loaded into your context at the start of every later session.

Usage:
- Save only durable, non-obvious facts the code and docs do not say (e.g. 'fidelity tests are slow; run X first'). Never save task progress, temporary details, secrets, credentials, or personal data.
- scope 'project' (default) is this workspace; 'global' is only for facts true in every project.
- One short line per note, at most 50 notes (8KB) per scope. view numbers the notes and remove takes that number; remove a note as soon as it is wrong.
- Changes take effect next session, not in this one.";

pub static PROMPT_TODO: &str = "\
Keep an ordered task list for multi-step work; the user sees it, and it survives compaction and resume.

Usage:
- Use it for 3 or more distinct steps or several requested tasks, not for trivial work or questions.
- Each call with todos replaces the whole list; call without todos to see it.
- Keep exactly one item in_progress. Mark each item completed as soon as it is done and verified, not in batches; if blocked, keep it in_progress and add an item for the blocker.
- Add steps you discover and remove ones that no longer apply.";

pub static PROMPT_WORKTREE: &str = "\
Review and land the work subagents and workflows did in their own git worktrees; their changes reach your working tree only when you merge them.

Usage:
- list shows every agent worktree in the repository, including other sessions'; act only on ids your own subagents or workflows reported.
- diff shows the stat summary, the patch (long patches are clipped; read the rest in the worktree path), and a static code review that does not run tests or linters, so run those in the worktree before merging.
- merge three-way merges into your working tree, uncommitted edits included, and removes the worktree; conflicting files get <<<<<<< markers to resolve with edit.
- Merge or discard every worktree your subagents create before you finish, or say why one is kept; unmerged worktrees are deleted after 24 hours. To get fixes first, continue the subagent with its task_id.";

pub static PROMPT_LSP: &str = "\
Query the language server for a file's language: diagnostics, definitions, references, hover, implementations, symbols, renames, code actions, and formatting.

Usage:
- Name a position with path, line (1-based, as shown by read or rg), and symbol (the identifier's text on that line).
- Locations come back grouped by file as `LINE:text` rows. symbols with path gives the file's outline; with query and no path it searches the workspace.
- rename, format, and code_actions with apply: true write files; check the files they list. Actions that only run a server command are refused.
- Servers start on first use, so the first query can take a few seconds. If no server handles a file the error says why; fall back to rg and read.";

pub static PROMPT_READ: &str = "\
Read a text file with line numbers, or attach an image.

Usage:
- Each line starts with its number and a tab; that prefix is not file content, so never copy it into edit oldText.
- Output stops at 2000 lines or 50KB with a `Use offset=N to continue.` note. For large files read a range with offset and limit, or one function or type with symbol.
- Images (png, jpeg, gif, webp, bmp) are attached; other binary files return a placeholder. Use fd, not read, to list a directory.";

pub static PROMPT_WRITE: &str = "\
Create a file or replace its entire content; parent directories are created.

Usage:
- text replaces the whole file, so include everything that should remain; change existing files with edit instead.
- When a language server handles the file, the result ends with its diagnostics.";

pub static PROMPT_EDIT: &str = "\
Edit an existing file with exact text replacements.

Usage:
- Each edits[] entry is one of:
  - {oldText, newText}: oldText must match exactly once, whitespace included; keep it as small as possible while unique.
  - {oldText, newText, replaceAll: true}: replace every occurrence, e.g. a rename.
  - {startText, endText, newText}: replace from startText through the first endText after it, inclusive; use it to rewrite or delete a block without copying it.
- Entries apply in order, each against the previous result. If any fails, nothing is written; fix it and resend them all.
- Put all changes to one file in one call, merging changes to the same lines into one entry; edit other files with separate calls in the same response.
- The result shows each changed region numbered like read, so there is no need to re-read. With a language server it also lists diagnostics: fix errors you introduced, but expect transient ones mid-change. Line endings are preserved.";

pub static PROMPT_FD: &str = "\
Find files and directories by path.

Usage:
- pattern is a smart-case regex matched anywhere in the workspace-relative path, not a glob; omit it to list everything under path.
- Results are grouped under their directory, and directories end with `/`. Ignored files are excluded; hidden: true adds dotfiles.
- Use depth for a bounded overview (depth 2 for top-level layout) and type to filter by language. stats adds line counts and sizes; with sort and min_lines it finds large files.";

pub static PROMPT_RG: &str = "\
Search file contents with a smart-case Rust regex (no lookaround or backreferences).

Usage:
- Escape metacharacters for literal text (e.g. `foo\\(`); use `a|b` for several terms.
- Matches print as `LINE:text` rows under each file path; pass them to read as path and offset. context adds surrounding lines; output_mode 'files' or 'count' surveys broad patterns cheaply.
- Matches stay within one line unless multiline: true.
- Ignored files, binary files, and files over 1 MB are skipped; hidden: true adds dotfiles. path may be a single file; use fd to find files by name.";

pub static PROMPT_BASH: &str = "\
Execute a bash command in the working directory and return combined stdout and stderr.

Usage:
- Each call is a fresh non-interactive shell already in the working directory, so do not cd: environment changes do not persist, stdin is closed, pagers and git prompts are disabled, and CI=true.
- Use it for git, builds, tests, and linters, not for reading or editing files.
- A command that exceeds timeout (default 30s) is not killed: it moves to the background and its output arrives later as a message. background: true does that immediately; false kills it on timeout. Background jobs are killed after 1 hour.
- Never poll with sleep, ps, or pgrep: keep working, call wait, or end your turn.
- Failures end with `[exit N]`. Long output keeps its first and last lines (more of the last on failure) and names the full log as `log: PATH` (search it with rg or page it with read), plus a summary of failing tests when it finds any. Pipe noisy commands through rg, head, or tail.";

pub static PROMPT_WEB_SEARCH: &str = "\
Search the web and return titles, URLs, and snippets.

Usage:
- Snippets are rarely authoritative: open the best results with web_fetch, preferring primary sources (official docs, release notes, upstream repositories).
- For recent events or latest versions, set recency and account for today's date.";

pub static PROMPT_WEB_FETCH: &str = "\
Fetch a URL and return its content as clean text (HTML, JSON, Markdown, text, CSV, XML, or PDF).

Usage:
- Output is paged by lines; pass offset to continue. Responses are cached briefly, so paging is cheap.
- JavaScript is not executed: client-rendered pages return an error. Private network addresses are blocked unless configured.
- Use it on URLs from the user or web_search; use read for local files.";

pub static PROMPT_SUBAGENT: &str = "\
Run a subagent in an isolated context for focused research, review, planning, or self-contained work.

Usage:
- Roles: 'scout' (default) explores read-only with read, fd, rg, lsp, web_fetch, and web_search; 'critic' reviews adversarially and 'planner' plans, both with read, fd, rg, and lsp; 'general' adds write, edit, undo, bash, todo, and wait and is the only role that changes files. Tools are limited to those enabled in this session.
- Subagents do not see your conversation: give the goal, relevant paths, constraints, and expected output in prompt or context_slice. inherit_context shares only the last 20 text messages, clipped.
- Always pass name, a short camelCase label (e.g. 'performanceAudit').
- In a git repository each subagent works in its own worktree, a snapshot of your working tree; its changes reach your files only when you merge them with the worktree tool. Calls in one response run in parallel (outside a git repository only read-only roles do); give parallel subagents disjoint files and non-conflicting commands.
- The call returns a task id at once and the result arrives later as a message; your turn waits for it. Keep working meanwhile and call wait only when blocked on it. To steer a running subagent, call again with its task_id and a prompt.
- Treat a result as a claim: review its changes and rerun checks yourself; a 'failed tool calls' footer means work may be missing. To fix, continue, or answer its question, call again with the same task_id; it keeps its history. Do not redo its work yourself.
- inspect: true lists your subagent tasks, or with task_id shows one task's full record; use it to recover task ids after compaction.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinToolKind {
    ReadOnly,
    WorkspaceMutation,
    Network,
    Shell,
    Composite,
    Memory,
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
        name: "undo",
        capability: BuiltinToolKind::WorkspaceMutation,
        prompt: PROMPT_UNDO,
        prompt_snippet: "Revert the last edit, write, or lsp change to a file, while the file still holds what that tool wrote",
        schema: generated_schema::<UndoArgs>,
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
        name: "lsp",
        capability: BuiltinToolKind::WorkspaceMutation,
        prompt: PROMPT_LSP,
        prompt_snippet: "Language-server diagnostics, definitions, references, hover, symbols, and renames for any configured language",
        schema: generated_schema::<LspArgs>,
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
    BuiltinToolDeclaration {
        name: "memory",
        capability: BuiltinToolKind::Memory,
        prompt: PROMPT_MEMORY,
        prompt_snippet: "Keep short notes across sessions (durable, non-obvious project facts; never secrets)",
        schema: generated_schema::<MemoryArgs>,
    },
    BuiltinToolDeclaration {
        name: "todo",
        capability: BuiltinToolKind::ReadOnly,
        prompt: PROMPT_TODO,
        prompt_snippet: "Track an ordered task list for multi-step work (pending, in_progress, completed)",
        schema: generated_schema::<TodoArgs>,
    },
    BuiltinToolDeclaration {
        name: "wait",
        capability: BuiltinToolKind::ReadOnly,
        prompt: PROMPT_WAIT,
        prompt_snippet: "Block until a background job or subagent task finishes, instead of polling with sleep",
        schema: generated_schema::<WaitArgs>,
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
