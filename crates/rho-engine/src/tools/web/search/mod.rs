pub mod brave;
pub mod ddg_lite;
pub mod engine;
pub mod firecrawl;
pub mod query;
pub mod result;
pub mod yahoo;

#[cfg(test)]
mod tests;

use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
use crate::tools::web::http::HttpClient;
use crate::tools::web::rate_limiter::SearchRateLimiter;
pub use engine::{EngineKind, EngineRequest, MultiEngineParams, search_multi_engine, search_single_engine};
pub use query::{
    build_search_query_with_filters, matches_domain_filters, matches_site, normalize_domain, normalize_domain_filters,
    relax_query,
};
pub use result::{FormatResultsParams, SearchResult, deduplicate_results, format_search_results};
pub use rho_harness_core::args::{WebSearchArgs, WebSearchRecency};
use rho_harness_core::error::AppError;
use rig::tool::{Tool, ToolContext, ToolExecutionError};

pub struct SearchQueryParams<'a> {
    pub query: &'a str,
    pub limit: usize,
    pub recency: Option<WebSearchRecency>,
    pub domains: Option<&'a [String]>,
}

pub struct WebSearchConfig {
    pub region: String,
    pub timeout_sec: u64,
}

#[derive(Clone)]
pub struct WebSearchTool {
    pub http: HttpClient,
    pub rate_limiter: SearchRateLimiter,
    pub region: String,
    pub timeout_sec: u64,
}

impl WebSearchTool {
    pub fn new(http: HttpClient, rate_limiter: SearchRateLimiter, config: WebSearchConfig) -> Self {
        Self {
            http,
            rate_limiter,
            region: config.region,
            timeout_sec: config.timeout_sec,
        }
    }

    pub async fn execute(&self, args: WebSearchArgs) -> Result<ToolResult, AppError> {
        let query = args.query.trim();
        if query.is_empty() {
            return Ok(ToolResult::error("Empty search query provided"));
        }

        let limit = args.limit.unwrap_or(5).clamp(1, 20);
        let domains = args.domains.as_deref();
        let recency = args.recency;
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();

        let effective_query = build_search_query_with_filters(query, domains);
        let results = self
            .search(SearchQueryParams {
                query: &effective_query,
                limit,
                recency,
                domains,
            })
            .await;

        if results.is_empty() {
            if domains.is_none() && recency.is_none() {
                let relaxed = relax_query(query);
                if relaxed != query {
                    let relaxed_results = self
                        .search(SearchQueryParams {
                            query: &relaxed,
                            limit,
                            recency: None,
                            domains: None,
                        })
                        .await;
                    if !relaxed_results.is_empty() {
                        return Ok(ToolResult::success(format_search_results(FormatResultsParams {
                            query,
                            results: &relaxed_results,
                            limit,
                            today: &today,
                        })));
                    }
                }
            }
            return Ok(ToolResult::success(format!(
                "No search results found for: \"{query}\" (searched on {today})"
            )));
        }

        Ok(ToolResult::success(format_search_results(FormatResultsParams {
            query,
            results: &results,
            limit,
            today: &today,
        })))
    }

    async fn search(&self, params: SearchQueryParams<'_>) -> Vec<SearchResult> {
        search_multi_engine(MultiEngineParams {
            http: &self.http,
            rate_limiter: &self.rate_limiter,
            region: &self.region,
            timeout_sec: self.timeout_sec,
            query: params.query,
            limit: params.limit,
            recency: params.recency,
            domains: params.domains,
        })
        .await
    }
}

impl Tool for WebSearchTool {
    const NAME: &'static str = "web_search";
    type Args = WebSearchArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Search the web and return structured search results with titles, summaries, and URLs.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<WebSearchArgs>()
    }

    async fn call(&self, _context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        into_rig_result(self.execute(args).await)
    }
}
