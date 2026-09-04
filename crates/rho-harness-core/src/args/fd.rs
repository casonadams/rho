use serde::{Deserialize, Serialize};

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
