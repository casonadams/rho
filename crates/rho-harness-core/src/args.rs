//! Host-side data shapes for built-in tool arguments.

use serde::{Deserialize, Serialize};

pub const DEFAULT_BASH_TIMEOUT_SEC: u64 = 30;
pub const DEFAULT_READ_LIMIT: usize = 2000;

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct BashArgs {
    /// Command to execute
    pub command: String,
    /// Timeout in seconds (default: 30)
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct EditReplacement {
    /// Exact text in the file to replace (must match exactly once)
    #[serde(rename = "oldText")]
    pub old_text: String,
    /// Replacement text
    #[serde(rename = "newText")]
    pub new_text: String,
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
    /// Include hidden entries and paths excluded by ignore rules (.gitignore, .ignore)
    pub hidden: Option<bool>,
    /// Maximum traversal depth, clamped to 1-10 when provided (default: unlimited)
    pub depth: Option<usize>,
    /// Maximum number of results to return (default: 200, max: 1000)
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
    /// Include hidden entries and paths excluded by ignore rules (.gitignore, .ignore)
    pub hidden: Option<bool>,
    /// Maximum number of matches to return (default: 200, max: 1000)
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct WebFetchArgs {
    /// URL to fetch
    pub url: String,
    /// Line number to start reading from (1-indexed, default 1)
    pub offset: Option<usize>,
    /// Maximum number of lines to return (default 200)
    pub limit: Option<usize>,
    /// Extraction mode ("auto", "main", or "full", default "auto")
    pub mode: Option<String>,
    /// Optional format override ("html", "json", "markdown", "csv", "xml", "pdf")
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
    /// Maximum number of search results to return (default: 5)
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
pub mod rg {
    pub use super::RgArgs;
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
