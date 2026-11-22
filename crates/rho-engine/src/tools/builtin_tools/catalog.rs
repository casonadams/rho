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
use rho_harness_core::args::{MemoryArgs, SubagentArgs, WorktreeArgs};

pub static PROMPT_MEMORY: &str = "\
Keep short notes that persist across sessions; saved notes are loaded into your context at the start of every later session.

Usage:
- Save only durable, non-obvious facts that would help a future session and that the code or docs do not already say: e.g. 'fidelity tests are slow; run X first', 'file Y is high risk'. Do not save task progress, obvious conventions, or anything temporary.
- Never save secrets, credentials, tokens, or personal data.
- scope 'project' (default) is for this workspace only; 'global' applies to every project, so use it only for facts that hold everywhere.
- Keep each note to one short line. Each scope holds at most 50 notes (8KB); when it is full, remove stale notes first.
- `view` lists the notes numbered per scope; `remove` takes that number as `index`. Remove a note as soon as it becomes wrong or obsolete.
- Notes you add or remove take effect in the next session; this session's context does not change.";

pub static PROMPT_WORKTREE: &str = "\
Review and land work that subagents and workflows did in their own git worktrees.

Usage:
- Every subagent and workflow works in its own worktree, a separate checkout outside your working tree. Their changes reach your working tree only when you merge them.
- `list` shows every agent worktree in this repository, oldest first, including ones from other sessions; act only on ids your own subagents or workflows reported. `diff` shows one worktree's stat summary and patch (long patches are clipped; read the files in the worktree path for the rest).
- Review before merging: read the diff, and run checks in the worktree directory if needed.
- `merge` applies the changes to your working tree with a three-way merge against your current files (uncommitted edits included), then removes the worktree. Files changed on both sides get <<<<<<< conflict markers: resolve them with edit, then re-run checks.
- `discard` deletes a worktree you do not want.
- You own the worktrees your subagents and workflows create: merge or discard each before you finish, or tell the user why one is kept. Unmerged worktrees are deleted 24 hours after they were created.
- To have a subagent fix its work before merging, continue its task with the subagent tool's `task_id`; it keeps working in the same worktree.";

pub static PROMPT_LSP: &str = "\
Query the language server for a file's language: compiler diagnostics, definitions, references, hover, implementations, symbols, and renames.

Usage:
- Name a position with path, line (1-based, as shown by read or rg), and symbol (the identifier's text on that line; its first whole-word occurrence is used). Prefer this over rg when you need the real definition or every use of a symbol, not text matches.
- definition, type_definition, implementation, and references return locations grouped by file as `LINE:text` rows.
- hover returns the symbol's type and documentation.
- symbols with path returns the file's outline as `START-END kind name` rows (pass query to filter by name); with query and no path it searches symbols across the workspace.
- diagnostics with path returns the file's current errors and warnings; without path it lists everything the running servers have reported.
- rename changes the symbol at the position to new_name in every file and writes them; review the listed files afterwards.
- status lists started servers and which configured servers are installed.
- Servers start on first use (reading a file also starts its server in the background), so the first query can take a few seconds. If no server handles a file the error says why (not installed, no project root marker); fall back to rg and read.";

pub static PROMPT_READ: &str = "\
Read a text file with line numbers, or attach an image.

Usage:
- Use read to examine files instead of cat or sed; path may be relative to the working directory or absolute.
- Each line is prefixed with its line number and a tab; the prefix is not part of the file, so never include it in edit oldText.
- Output is capped at 2000 lines or 50KB, whichever comes first. A trailing note ending in `Use offset=N to continue.` gives the next offset; pass it, optionally with limit, to page through large files. A single line longer than 50KB is shown cut to its first 50KB, and an empty file returns `[File is empty]`.
- For a known region, read it with offset and limit instead of the whole file. To read one function, type, or method, pass symbol (e.g. 'parse', 'Config.load') instead: the file's language server finds its lines.
- Read a file before editing it so oldText matches exactly. Reads run in parallel when every call in a response is read-only, so read independent files together.
- Supported images (png, jpeg, gif, webp, bmp) are attached to the result instead of being returned as text. Binary files return a short placeholder, and other non-UTF-8 files return an error. Use fd, not read, to list a directory.";

pub static PROMPT_WRITE: &str = "\
Create a file or completely overwrite an existing one. Parent directories are created automatically.

Usage:
- Use write only for new files or complete rewrites; content replaces the whole file, so include everything that should remain.
- For small or targeted changes to existing files, use edit instead.
- Use write instead of shell redirects, heredocs, or scripts to create files.
- When a language server handles the file, the result ends with its errors and warnings for the new content (`LSP (...)` rows of `LINE:COL severity message`).";

pub static PROMPT_EDIT: &str = "\
Edit an existing file with exact text replacements, applied in order.

Usage:
- Each edits[] entry is one of:
  - {oldText, newText}: oldText must match exactly once, including whitespace and indentation; keep it as small as possible while still unique.
  - {oldText, newText, replaceAll: true}: replace every occurrence, e.g. a rename within the file.
  - {startText, endText, newText}: replace the region from startText through the first endText after it, inclusive; startText must match exactly once. Use this to delete or rewrite a block without copying it into oldText.
- Entries apply sequentially, so each one matches against the result of the previous entry. If any entry fails, nothing is written; fix that entry and resend all of them.
- Put all changes to one file in one call; for changes across files, make one edit call per file in the same response.
- If two changes touch the same lines, merge them into one entry instead of emitting overlapping edits.
- Line endings are matched and preserved automatically; to create a new file use write.
- The result lists each changed region of the new file numbered like read, so you can check the change without re-reading. When a language server handles the file, the result ends with its errors and warnings for the new content; fix errors you introduced, but expect transient ones while a multi-step change is half done.
- Use edit for every change to an existing file; never sed -i, perl -pi, python or awk scripts, or shell redirects.";

pub static PROMPT_FD: &str = "\
Find files and directories by workspace-relative path pattern.

Usage:
- pattern is an optional smart-case regex (case-insensitive unless it contains an uppercase character) matched unanchored against workspace-relative paths, not a glob; if omitted, all entries in the search root match.
- Files and directories both match; directories end with `/`. Output starts with the total, then lists entries grouped under their parent directory (`dir/` header, one indented name per line; `./` is the root). A directory whose entries are listed appears only as their header and is not counted. The search path itself is not listed, and depth counts from it. Entries are sorted by directory, then name.
- Output fills a 20KB page; when more remain, the last line names the exact next offset. Pass offset to page through every result, or narrow with path, pattern, type, or depth. limit caps results per page.
- Hidden entries and ignore rules (.gitignore, .ignore, global git excludes) are respected; hidden: true adds hidden dotfiles, but ignored paths stay excluded.
- Use type (e.g. 'rust', 'py') to filter by file type and depth (1-10) for a bounded overview, e.g. depth 2 for top-level layout.
- Set stats: true for a table of line counts, byte sizes, and full paths; use min_lines or max_lines to filter by line count (e.g. min_lines: 150 to identify oversized files), and sort ('lines' or 'size', both descending, or 'path') to order results.";

pub static PROMPT_RG: &str = "\
Search file contents with a regex and return line-oriented matches.

Usage:
- pattern is a smart-case regex (case-insensitive unless it contains an uppercase character) matched against each line; escape regex metacharacters (e.g. `\\(`, `\\.`) when searching for literal text, and use `a|b` to search several terms at once.
- Output starts with the total (`N matches in M files`). In the default output_mode 'content', matches are grouped by file: the path on its own line, then one `LINE:text` row per match, sorted by line, with a blank line between files; pass the path and LINE to read as path and offset. Match lines are truncated at 500 characters.
- Set context (0-10) to see lines around each match without a follow-up read: context lines print as `LINE-text` and `--` separates non-adjacent blocks. Paging still counts matches.
- output_mode 'files' lists only the matching file paths and 'count' adds `:N` matches per file, both grouped under their parent directory; use them to survey a broad pattern before reading lines.
- Output fills a 20KB page; when more remain, the last line names the exact next offset. Pass offset to page through every result, or narrow with path or type. limit caps results per page.
- Hidden entries and ignore rules (.gitignore, .ignore, global git excludes) are respected; hidden: true adds hidden dotfiles, but ignored paths stay excluded.
- Regex syntax is Rust regex: no lookaround or backreferences. Matches stay within one line unless multiline: true, which lets `\n` in the pattern span lines.
- Binary files and files over 1 MB are skipped; use type (e.g. 'rust', 'py') to filter by file type.
- path may be a single file. Use fd instead to find files by name.";

pub static PROMPT_BASH: &str = "\
Execute a bash command in the working directory and return combined stdout and stderr.

Usage:
- Commands run directly in the working directory; do not prefix commands with cd. Each call is a fresh, non-interactive shell: environment changes do not persist, stdin is closed, pagers and git prompts are disabled, and CI=true is set.
- Use fd for file discovery, rg for content search; use bash for git actions, cargo builds, tests, and linters.
- Never read files with cat, sed, or awk, and never create or modify files with bash (sed -i, perl -pi, python or awk scripts, echo or cat redirects, tee, mv over a file); use read, edit, and write. Project formatters and generators (cargo fmt, make) are fine.
- timeout is in seconds (default 30). A command that exceeds it is NOT killed: it moves to a background job and its output arrives later as a follow-up message. Set background: false to kill it on timeout instead.
- Set background: true for long-running work (full builds, test suites, servers); you get a job id immediately and the output is delivered when the job finishes. Never poll with sleep, ps, or pgrep. Background jobs are killed after 1 hour.
- Non-zero exit codes return an error result ending in `[exit N]`. Successful output over 50 lines shows only the first 10 and last 15 lines, and long or truncated output ends with `log: PATH`, the full output file to search with rg or page with read. When clipped output has failing tests (cargo test, nextest, pytest, go test, Jest, Vitest), a `[N failing tests; details from log]` section above the last line lists each with its key failure lines. Filter output with rg, head, or tail instead of dumping everything.";

pub static PROMPT_WEB_SEARCH: &str = "\
Search the web and return structured summaries and URLs.

Usage:
- Prefer web_search for finding public documentation, repositories, package releases, and technical references; then open the best results with web_fetch, since snippets alone are rarely authoritative.
- Returns concise result summaries with title, URL, and snippet (default 5 results, max 20; raise limit for broader surveys).
- Use recency ('day', 'week', 'month', 'year') to filter results by freshness; factor in today's date when asking about latest versions.
- Use domains to limit results to specific domains (e.g. ['github.com']) or exclude domains with a leading '-' (e.g. ['-spam.com']).
- Prefer primary sources (official docs, release notes, upstream repositories).";

pub static PROMPT_WEB_FETCH: &str = "\
Fetch HTML, JSON, Markdown, text, CSV, XML, or PDF content from a URL and return clean text.

Usage:
- Extracts clean markdown/text without navigation bloat or HTML tags; the format is detected automatically, and format overrides it when detection is wrong.
- Output is paginated by lines (default limit 200): pass offset to read further pages of long documents.
- mode 'auto' (default) extracts the main article content when the page has a clear one and falls back to the full page; 'main' requires main content and errors when there is none; use mode: 'full' when navigation, sidebars, or other page chrome are needed.
- JavaScript is not executed: pages that render client-side return an error saying so. Private network addresses are blocked unless configured.
- Use it on URLs from the user or from web_search results; for local files use read.
- Responses are cached briefly, so paging with offset or refetching the same URL is cheap.";

pub static PROMPT_SUBAGENT: &str = "\
Spawn a specialized subagent in an isolated context to perform focused research, code review, or planning.

Usage:
- Use role 'scout' for read-only codebase exploration, searching files, and locating symbols without polluting parent context.
- Use role 'critic' for adversarial code review, bug hunting, and invariant verification.
- Use role 'planner' to draft execution steps or trade-off analyses without mutating files.
- Use role 'general' for self-contained multi-step tasks.
- Default role is 'scout'. Tools by role (limited to tools enabled in this session): scout has read, fd, rg, web_fetch, web_search; critic and planner have read, fd, rg; general has all of those plus write, edit, and bash. Only 'general' can change files, so give it a precise, self-contained prompt (goal, relevant paths, constraints, expected output).
- In a git repository each subagent works in its own worktree, a snapshot of your working tree including uncommitted edits. Its changes do not appear in your files until you review and merge them with the worktree tool.
- Subagents start with no memory of your conversation, so put everything they need in `prompt` or `context_slice`. `inherit_context` shares only the last 20 text messages, each clipped to 1,500 characters.
- Subagent calls in the same response run in parallel: all of them in a git repository, where each has its own worktree, otherwise only new read-only ones (scout, critic, planner). Parallel subagents still share the machine (ports, installed dependencies, build caches), so give them non-conflicting commands, and give two subagents disjoint files to avoid merge conflicts.
- Subagents run up to `turns` model calls (default 100) on the `model` role you pass ('default', 'smol', or 'slow'); without one they use the role or agent's model configured by the user, else 'smol' when it is configured, otherwise the session model. They return a concise summary to the parent.
- A subagent still running after the foreground budget (60 seconds unless configured) moves to the background: the call returns its task id at once, its result arrives later as a follow-up message, and your turn waits for it before ending. Meanwhile continue the parts of the task that do not depend on it instead of waiting, and do not inspect that task until its result arrives. To redirect a running subagent, call subagent with its `task_id` and a `prompt` (optionally `context_slice`): the message is queued and reaches it before its next model call. Omit `model` and `turns`, which cannot change mid-round.
- Always pass `name`: a short camelCase label for the task (e.g. 'performanceAudit', 'retryAfterReview'); it identifies the subagent in progress lines, /agents, its task id, and its result.
- Each result ends with a task id and state. Treat the answer as a claim, not proof: before accepting, review its changes (worktree diff) and re-run the relevant checks yourself. If something was missed or wrong, call subagent again with that `task_id` and your feedback (e.g. 'you missed X, fix it'); the subagent continues with everything it already read and did. Repeat until the work is right.
- Results list 'Files changed' (its write/edit calls), 'Also changed in the workspace' (bash, formatters, other agents), and 'Tool outcomes' (errors and permission denials). A footer with 'failed tool calls' means part of the work may not have happened even if the answer says it did.
- Pass `inspect: true` without `task_id` to list the session's subagent tasks (use it to recover task ids after compaction); with a `task_id` it shows that task's messages, final answer, every tool call with its outcome, and its files, without running it.
- State 'input-required' means the subagent is waiting: it asked you a question ('Question for you: ...') or stopped early (turn limit or no final answer) with a summary. Reply with the same `task_id` and your answer or 'continue'. Do not redo its work yourself.
- State 'failed' may still report findings and 'Files changed'; build on them or send the `task_id` follow-up instead of starting over.";

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
