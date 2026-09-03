use serde::{Deserialize, Serialize};

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
