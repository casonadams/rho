pub mod brave;
pub mod ddg_lite;
pub mod engine;
pub mod exa;
pub mod format;
pub mod query;
pub mod result;
pub mod yahoo;

#[cfg(test)]
mod tests;

use crate::tools::engine_tool::TypedTool;
use crate::tools::types::ToolResult;
use crate::tools::web::http::HttpClient;
use crate::tools::web::rate_limiter::SearchRateLimiter;
pub use engine::{EngineKind, EngineRequest, MultiEngineParams, default_engine_chain, resolve_engine_chain};
pub use format::{FormatResultsParams, format_search_results};
pub use query::{build_search_query_with_filters, matches_domain_filters, normalize_domain_filters, relax_query};
pub use result::{SearchResult, deduplicate_results};
pub use rho_harness_core::args::{WebSearchArgs, WebSearchRecency};
use rho_harness_core::error::AppError;

#[derive(Clone)]
pub struct WebSearchConfig {
    pub region: String,
    pub timeout_sec: u64,
    pub engines: Vec<EngineKind>,
    pub auth_file: Option<std::path::PathBuf>,
}

#[derive(Clone)]
pub struct WebSearchTool {
    pub http: HttpClient,
    pub rate_limiter: SearchRateLimiter,
    pub config: WebSearchConfig,
}

impl WebSearchTool {
    pub fn new(http: HttpClient, rate_limiter: SearchRateLimiter, mut config: WebSearchConfig) -> Self {
        if config.engines.is_empty() {
            config.engines = default_engine_chain();
        }
        Self {
            http,
            rate_limiter,
            config,
        }
    }

    async fn perform_search(
        &self,
        query: &str,
        limit: usize,
        domains: Option<&[String]>,
        recency: Option<WebSearchRecency>,
    ) -> engine::MultiEngineOutcome {
        engine::search_multi_engine_outcome(MultiEngineParams {
            http: &self.http,
            rate_limiter: &self.rate_limiter,
            region: &self.config.region,
            timeout_sec: self.config.timeout_sec,
            query,
            limit,
            recency,
            domains,
            engines: &self.config.engines,
            auth_file: self.config.auth_file.as_deref(),
        })
        .await
    }

    async fn handle_empty_search(
        &self,
        query: &str,
        today: &str,
        limit: usize,
        no_filters: bool,
        last_error: Option<AppError>,
    ) -> ToolResult {
        let relaxed = if no_filters {
            try_relaxed_search(query, limit, today, |relaxed: String| async move {
                self.perform_search(&relaxed, limit, None, None).await
            })
            .await
        } else {
            Ok(None)
        };
        empty_search_result(query, today, relaxed, last_error)
    }

    pub async fn execute(&self, args: WebSearchArgs) -> Result<ToolResult, AppError> {
        let query = args.query.trim();
        if query.is_empty() {
            return Ok(ToolResult::error("Empty search query provided"));
        }

        let limit = args.limit.unwrap_or(5).clamp(1, 20);
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let outcome = self
            .perform_search(query, limit, args.domains.as_deref(), args.recency)
            .await;

        if outcome.results.is_empty() {
            let no_filters = args.domains.is_none() && args.recency.is_none();
            let res = self
                .handle_empty_search(query, &today, limit, no_filters, outcome.last_error)
                .await;
            return Ok(res);
        }

        Ok(ToolResult::success(format_search_results(FormatResultsParams {
            query,
            results: &outcome.results,
            limit,
            today: &today,
        })))
    }
}

/// What an empty search reports: relaxed results, else the first search's error, else the retry's
/// error, else that nothing was found.
fn empty_search_result(
    query: &str,
    today: &str,
    relaxed: Result<Option<ToolResult>, AppError>,
    last_error: Option<AppError>,
) -> ToolResult {
    match (relaxed, last_error) {
        (Ok(Some(found)), _) => found,
        (_, Some(err)) | (Err(err), None) => ToolResult::error(err.to_string()),
        (Ok(None), None) => ToolResult::success(format!(
            "No search results found for: \"{query}\" (searched on {today})"
        )),
    }
}

/// Retries `query` relaxed; `None` when it cannot be relaxed or the retry also finds nothing.
async fn try_relaxed_search<F, Fut>(
    query: &str,
    limit: usize,
    today: &str,
    search: F,
) -> Result<Option<ToolResult>, AppError>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = engine::MultiEngineOutcome>,
{
    let relaxed = relax_query(query);
    if relaxed == query {
        return Ok(None);
    }
    let outcome = search(relaxed.clone()).await;
    if let Some(err) = outcome.last_error {
        return Err(err);
    }
    if outcome.results.is_empty() {
        return Ok(None);
    }
    let header = format!("No search results found for: \"{query}\". Results for relaxed query \"{relaxed}\":");
    let body = format_search_results(FormatResultsParams {
        query: &relaxed,
        results: &outcome.results,
        limit,
        today,
    });
    Ok(Some(ToolResult::success(format!("{header}\n\n{body}"))))
}

#[async_trait::async_trait]
impl TypedTool for WebSearchTool {
    type Args = WebSearchArgs;
    const NAME: &'static str = "web_search";
    const CONCURRENCY_SAFE: bool = true;

    async fn run(&self, args: WebSearchArgs) -> Result<ToolResult, AppError> {
        self.execute(args).await
    }
}
