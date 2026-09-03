use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct FdArgs {
    /// Smart-case regex matched unanchored against each entry's workspace-relative path (case-insensitive unless it contains an uppercase character)
    pub pattern: String,
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
}
