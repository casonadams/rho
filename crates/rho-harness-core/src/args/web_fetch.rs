use serde::{Deserialize, Serialize};

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
