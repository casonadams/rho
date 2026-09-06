use crate::tools::bash::BashArgs;
use crate::tools::edit::EditArgs;
use crate::tools::fd::FdArgs;
use crate::tools::read::ReadArgs;
use crate::tools::rg::RgArgs;
use crate::tools::types::generated_schema;
use crate::tools::web::fetch::WebFetchArgs;
use crate::tools::web::search::WebSearchArgs;
use crate::tools::write::WriteArgs;

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
- For small or targeted changes to existing files, prefer edit instead.";

pub static PROMPT_EDIT: &str = "\
Make precise file edits with exact text replacement.

Usage:
- Every edits[].oldText must match a unique, non-overlapping region of the original file.
- If two changes affect the same block or nearby lines, merge them into one edit instead of emitting overlapping edits.
- Keep edits[].oldText as small as possible while still being unique in the file.
- Do not include large unchanged regions just to connect distant changes.";

pub static PROMPT_FD: &str = "\
Find files and directories by workspace-relative path pattern.

Usage:
- pattern is an optional smart-case regex (case-insensitive unless it contains an uppercase character) matched against workspace-relative paths; if omitted, all entries in the search root match.
- Files and directories both match; results are sorted lexicographically by default and capped at limit (default 200, max 1000) with a 20,000-entry collection ceiling; output is byte-capped at 50KB.
- Ignore rules (.gitignore, .ignore) and hidden entries are respected by default; set hidden: true to include both.
- Use type (e.g. 'rust', 'py') to filter by file type and depth (1-10) for a bounded overview, e.g. depth 2 for top-level layout.
- Set stats: true to include line counts and byte sizes; use min_lines or max_lines to filter by line count (e.g. min_lines: 150 to identify oversized files), and sort ('lines', 'size', or 'path') to order results.";

pub static PROMPT_RG: &str = "\
Search file contents with line-oriented results.

Usage:
- pattern is a smart-case regex (case-insensitive unless it contains an uppercase character) matched against file contents.
- Returns path:line: text lines sorted by path then line number; match lines are truncated at 500 characters.
- Results are capped at limit (default 200, max 1000) with a 5,000-match collection ceiling; output is byte-capped at 50KB.
- Ignore rules (.gitignore, .ignore) and hidden entries are respected by default; set hidden: true to include both.
- Binary files and files over 1 MB are skipped; use type (e.g. 'rust', 'py') to filter by file type.";

pub static PROMPT_BASH: &str = "\
Execute bash commands in the current working directory.

Usage:
- Commands run directly in the working directory; do not prefix commands with cd.
- Use fd for file discovery, rg for content search; use bash for git actions, cargo builds, tests, and linters.
- Use read/edit instead of sed, awk, or cat for reading and editing code.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinToolKind {
    ReadOnly,
    WorkspaceMutation,
    Network,
    Shell,
}

#[derive(Debug, Clone, Copy)]
pub struct BuiltinToolDeclaration {
    pub name: &'static str,
    pub capability: BuiltinToolKind,
    pub description: &'static str,
    pub prompt: &'static str,
    pub prompt_snippet: Option<&'static str>,
    pub prompt_guidelines: &'static [&'static str],
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
        description: "Read file contents with line numbering, offset, and limit safeguards.",
        prompt: PROMPT_READ,
        prompt_snippet: Some("Read file contents (with line numbering, offset, and limit safeguards)"),
        prompt_guidelines: &[
            "Use read to examine files instead of cat or sed",
            "Use offset and limit when reading large files",
        ],
        schema: generated_schema::<ReadArgs>,
    },
    BuiltinToolDeclaration {
        name: "write",
        capability: BuiltinToolKind::WorkspaceMutation,
        description: "Write full content to a file, automatically creating parent directories.",
        prompt: PROMPT_WRITE,
        prompt_snippet: Some("Create or overwrite files (automatically creates parent directories)"),
        prompt_guidelines: &["Use write only for new files or complete rewrites"],
        schema: generated_schema::<WriteArgs>,
    },
    BuiltinToolDeclaration {
        name: "edit",
        capability: BuiltinToolKind::WorkspaceMutation,
        description: "Edit a file by applying exact string replacements. Every oldText must match exactly once.",
        prompt: PROMPT_EDIT,
        prompt_snippet: Some(
            "Make precise file edits with exact text replacement (every edits[].oldText must match uniquely)",
        ),
        prompt_guidelines: &[
            "Use edit for precise changes (edits[].oldText must match exactly)",
            "When changing multiple separate locations in one file, use one edit call with multiple entries in edits[] instead of multiple edit calls",
            "Keep edits[].oldText as small as possible while still being unique in the file",
        ],
        schema: generated_schema::<EditArgs>,
    },
    BuiltinToolDeclaration {
        name: "bash",
        capability: BuiltinToolKind::Shell,
        description: "Execute a shell command in the current working directory with a timeout. Do not prefix commands with cd.",
        prompt: PROMPT_BASH,
        prompt_snippet: Some("Execute bash commands in the current working directory"),
        prompt_guidelines: &[
            "Use fd/rg for discovery and content search; bash for git actions, cargo builds, and tests",
            "Commands run directly in the working directory; do not prefix commands with cd",
        ],
        schema: generated_schema::<BashArgs>,
    },
    BuiltinToolDeclaration {
        name: "fd",
        capability: BuiltinToolKind::ReadOnly,
        description: "Find files and directories by workspace-relative path with a smart-case regex; gitignore-aware and bounded.",
        prompt: PROMPT_FD,
        prompt_snippet: Some("Find files and directories by path pattern (gitignore-aware, bounded)"),
        prompt_guidelines: &[
            "Use fd for file discovery instead of find, glob, or ls round-trips",
            "Use depth (1-10) with pattern '.' for a bounded workspace overview",
        ],
        schema: generated_schema::<FdArgs>,
    },
    BuiltinToolDeclaration {
        name: "rg",
        capability: BuiltinToolKind::ReadOnly,
        description: "Search file contents with a smart-case regex; gitignore-aware, skips binary and large files, bounded.",
        prompt: PROMPT_RG,
        prompt_snippet: Some("Search file contents by pattern (gitignore-aware, bounded)"),
        prompt_guidelines: &[
            "Use rg for content search instead of grep or bash pipelines",
            "Narrow with path or type when a pattern matches too much",
        ],
        schema: generated_schema::<RgArgs>,
    },
    BuiltinToolDeclaration {
        name: "web_search",
        capability: BuiltinToolKind::Network,
        description: "Search the web and return structured search results with titles, summaries, and URLs.",
        prompt: PROMPT_WEB_SEARCH,
        prompt_snippet: Some("Search the web and return structured summaries and URLs"),
        prompt_guidelines: &[],
        schema: generated_schema::<WebSearchArgs>,
    },
    BuiltinToolDeclaration {
        name: "web_fetch",
        capability: BuiltinToolKind::Network,
        description: "Fetch and extract readable content from a URL (HTML, JSON, Markdown, RSS/Atom, CSV, PDF).",
        prompt: PROMPT_WEB_FETCH,
        prompt_snippet: Some("Fetch and extract clean text or markdown from URLs"),
        prompt_guidelines: &[],
        schema: generated_schema::<WebFetchArgs>,
    },
];
