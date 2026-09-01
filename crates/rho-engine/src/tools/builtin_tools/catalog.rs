use crate::tools::ask_user::AskUserArgs;
use crate::tools::bash::BashArgs;
use crate::tools::edit::EditArgs;
use crate::tools::read::ReadArgs;
use crate::tools::types::generated_schema;
use crate::tools::web::fetch::FetchArgs;
use crate::tools::web::search::SearchArgs;
use crate::tools::write::WriteArgs;

pub static PROMPT_READ: &str = "\
Read file contents with offset and limit safeguards.

Usage:
- Use read to examine files instead of cat or sed.
- Use offset and limit when reading large files.
- Truncates lines when output exceeds maximum byte bounds.";

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

pub static PROMPT_BASH: &str = "\
Execute bash commands in the current working directory.

Usage:
- Commands run directly in the working directory; do not prefix commands with cd.
- Use bash for file discovery, git actions, cargo builds, tests, and linters.
- Use read/edit instead of sed, awk, or cat for reading and editing code.
- Captures combined stdout and stderr with output truncation safeguards.";

pub static PROMPT_ASK_USER: &str = "\
Ask the user one or more structured questions during execution.

When to use:
- Gather user preferences, constraints, or requirements when instructions are ambiguous or underspecified.
- Get confirmation on implementation trade-offs or architectural directions before making substantial changes.
- Offer choices to the user about what direction to take when multiple valid approaches exist.

Guidelines:
- Each question should have a concise header (1-3 words).
- Provide clear, mutually exclusive options (2-4 choices).
- Group all blocking questions into a single ask_user_question call.";

pub static PROMPT_WEBSEARCH: &str = "\
Search the web and return structured summaries and URLs.

Usage:
- Prefer websearch for finding public documentation, repositories, package releases, and technical references.
- Use recency ('day', 'week', 'month', 'year') to filter results by freshness.
- Use domains to limit results to specific domains (e.g. ['github.com']) or exclude domains with a leading '-' (e.g. ['-spam.com']).
- Returns concise result summaries with title, URL, and snippet.";

pub static PROMPT_WEBFETCH: &str = "\
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
    Interactive,
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
            "Use bash for file operations like ls, rg, find",
            "Commands run directly in the working directory; do not prefix commands with cd",
        ],
        schema: generated_schema::<BashArgs>,
    },
    BuiltinToolDeclaration {
        name: "search",
        capability: BuiltinToolKind::Network,
        description: "Search the web and return structured search results with titles, summaries, and URLs.",
        prompt: PROMPT_WEBSEARCH,
        prompt_snippet: Some("Search the web and return structured summaries and URLs"),
        prompt_guidelines: &[],
        schema: generated_schema::<SearchArgs>,
    },
    BuiltinToolDeclaration {
        name: "websearch",
        capability: BuiltinToolKind::Network,
        description: "Search the web and return structured search results with titles, summaries, and URLs.",
        prompt: PROMPT_WEBSEARCH,
        prompt_snippet: None,
        prompt_guidelines: &[],
        schema: generated_schema::<SearchArgs>,
    },
    BuiltinToolDeclaration {
        name: "web_search",
        capability: BuiltinToolKind::Network,
        description: "Search the web and return structured search results with titles, summaries, and URLs.",
        prompt: PROMPT_WEBSEARCH,
        prompt_snippet: None,
        prompt_guidelines: &[],
        schema: generated_schema::<SearchArgs>,
    },
    BuiltinToolDeclaration {
        name: "fetch",
        capability: BuiltinToolKind::Network,
        description: "Fetch and extract readable content from a URL (HTML, JSON, Markdown, RSS/Atom, CSV, PDF).",
        prompt: PROMPT_WEBFETCH,
        prompt_snippet: Some("Fetch and extract clean text or markdown from URLs"),
        prompt_guidelines: &[],
        schema: generated_schema::<FetchArgs>,
    },
    BuiltinToolDeclaration {
        name: "webfetch",
        capability: BuiltinToolKind::Network,
        description: "Fetch and extract readable content from a URL (HTML, JSON, Markdown, RSS/Atom, CSV, PDF).",
        prompt: PROMPT_WEBFETCH,
        prompt_snippet: None,
        prompt_guidelines: &[],
        schema: generated_schema::<FetchArgs>,
    },
    BuiltinToolDeclaration {
        name: "web_fetch",
        capability: BuiltinToolKind::Network,
        description: "Fetch and extract readable content from a URL (HTML, JSON, Markdown, RSS/Atom, CSV, PDF).",
        prompt: PROMPT_WEBFETCH,
        prompt_snippet: None,
        prompt_guidelines: &[],
        schema: generated_schema::<FetchArgs>,
    },
    BuiltinToolDeclaration {
        name: "ask",
        capability: BuiltinToolKind::Interactive,
        description: "Ask the user one or more structured questions to clarify ambiguous requirements, confirm architectural choices, or gather user preferences.",
        prompt: PROMPT_ASK_USER,
        prompt_snippet: Some(
            "Ask the user one or more structured questions to clarify ambiguous requirements, confirm architectural choices, or gather user preferences",
        ),
        prompt_guidelines: &[
            "Use ask whenever the user's request is underspecified, ambiguous, has multiple architectural trade-offs, or requires decisions that only the user can make. Do not make unconfirmed assumptions on critical design decisions.",
            "When asking questions, provide structured options with clear trade-offs and recommendations",
            "When unresolved user decisions block progress, ask them together in one ask call",
        ],
        schema: generated_schema::<AskUserArgs>,
    },
    BuiltinToolDeclaration {
        name: "ask_user",
        capability: BuiltinToolKind::Interactive,
        description: "Ask the user one or more structured questions to clarify ambiguous requirements, confirm architectural choices, or gather user preferences.",
        prompt: PROMPT_ASK_USER,
        prompt_snippet: None,
        prompt_guidelines: &[],
        schema: generated_schema::<AskUserArgs>,
    },
    BuiltinToolDeclaration {
        name: "ask_user_question",
        capability: BuiltinToolKind::Interactive,
        description: "Ask the user one or more structured questions to clarify ambiguous requirements, confirm architectural choices, or gather user preferences.",
        prompt: PROMPT_ASK_USER,
        prompt_snippet: None,
        prompt_guidelines: &[],
        schema: generated_schema::<AskUserArgs>,
    },
];
