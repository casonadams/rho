use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct SearchArgs {
    /// Search query
    pub query: String,
    /// Maximum number of search results to return (default: 5)
    pub limit: Option<usize>,
}
