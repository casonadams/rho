use serde::{Deserialize, Serialize};

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
