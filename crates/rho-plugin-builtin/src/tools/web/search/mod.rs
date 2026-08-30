pub mod brave;
pub mod ddg_lite;
pub mod firecrawl;
pub mod result;
pub mod yahoo;

use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
use crate::tools::web::http::{BRAVE_CHROME_UA, HttpClient, HttpRequest, LYNX_UA};
use crate::tools::web::rate_limiter::SearchRateLimiter;
use rand::seq::SliceRandom;
use result::SearchResult;
pub use rho_core::args::SearchArgs;
use rho_core::error::AppError;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use std::collections::HashSet;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineKind {
    Brave,
    DuckDuckGoLite,
    Yahoo,
    Firecrawl,
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

    pub async fn execute(&self, args: SearchArgs) -> Result<ToolResult, AppError> {
        let query = args.query.trim();
        if query.is_empty() {
            return Ok(ToolResult::error("Empty search query provided"));
        }

        let limit = args.limit.unwrap_or(5).clamp(1, 20);

        // Perform rate-limited multi-engine search
        let results = self.search_multi_engine(query, limit).await;

        if results.is_empty() {
            // Try query relaxation
            let relaxed = relax_query(query);
            if relaxed != query {
                let relaxed_results = self.search_multi_engine(&relaxed, limit).await;
                if !relaxed_results.is_empty() {
                    return Ok(ToolResult::success(format_search_results(&relaxed_results, limit)));
                }
            }
            return Ok(ToolResult::success(format!("No search results found for: \"{query}\"")));
        }

        Ok(ToolResult::success(format_search_results(&results, limit)))
    }

    async fn search_multi_engine(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        let engines = {
            let mut list = vec![
                EngineKind::Brave,
                EngineKind::DuckDuckGoLite,
                EngineKind::Yahoo,
                EngineKind::Firecrawl,
            ];
            let mut rng = rand::thread_rng();
            list.shuffle(&mut rng);
            list
        };

        for engine in engines {
            self.rate_limiter.acquire().await;
            let res = match engine {
                EngineKind::Brave => self.search_brave(query).await,
                EngineKind::DuckDuckGoLite => self.search_ddg_lite(query).await,
                EngineKind::Yahoo => self.search_yahoo(query).await,
                EngineKind::Firecrawl => self.search_firecrawl(query).await,
            };

            if let Ok(results) = res {
                let deduplicated = deduplicate_results(results);
                if !deduplicated.is_empty() {
                    return deduplicated.into_iter().take(limit).collect();
                }
            }
        }

        Vec::new()
    }

    async fn search_brave(&self, query: &str) -> Result<Vec<SearchResult>, AppError> {
        let url = format!(
            "https://search.brave.com/search?q={}&source=web",
            urlencoding_encode(query)
        );
        let (html, _) = self
            .http
            .get_text(HttpRequest {
                url: &url,
                user_agent: Some(BRAVE_CHROME_UA),
                timeout_sec: self.timeout_sec,
                max_bytes: 2_000_000,
            })
            .await?;
        Ok(brave::parse_brave_html(&html))
    }

    async fn search_ddg_lite(&self, query: &str) -> Result<Vec<SearchResult>, AppError> {
        let url = format!(
            "https://lite.duckduckgo.com/lite/?q={}&kl={}",
            urlencoding_encode(query),
            urlencoding_encode(&self.region)
        );
        let (html, _) = self
            .http
            .get_text(HttpRequest {
                url: &url,
                user_agent: Some(LYNX_UA),
                timeout_sec: self.timeout_sec,
                max_bytes: 2_000_000,
            })
            .await?;
        Ok(ddg_lite::parse_ddg_lite_html(&html))
    }

    async fn search_yahoo(&self, query: &str) -> Result<Vec<SearchResult>, AppError> {
        let url = format!("https://search.yahoo.com/search?p={}", urlencoding_encode(query));
        let (html, _) = self
            .http
            .get_text(HttpRequest {
                url: &url,
                user_agent: Some(LYNX_UA),
                timeout_sec: self.timeout_sec,
                max_bytes: 2_000_000,
            })
            .await?;
        Ok(yahoo::parse_yahoo_html(&html))
    }

    async fn search_firecrawl(&self, query: &str) -> Result<Vec<SearchResult>, AppError> {
        let payload = serde_json::json!({
            "query": query,
            "limit": 10,
            "sources": ["web"]
        });

        let resp = self
            .http
            .client
            .post("https://api.firecrawl.dev/v2/search")
            .json(&payload)
            .timeout(std::time::Duration::from_secs(self.timeout_sec))
            .send()
            .await
            .map_err(|e| AppError::Tool(format!("Firecrawl request failed: {e}")))?;

        if resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            Ok(firecrawl::parse_firecrawl_json(&body))
        } else {
            Err(AppError::Tool("Firecrawl search error".to_string()))
        }
    }
}

fn urlencoding_encode(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}

fn relax_query(query: &str) -> String {
    query
        .replace(['"', '\'', '(', ')', '[', ']', '+'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn deduplicate_results(results: Vec<SearchResult>) -> Vec<SearchResult> {
    let mut seen_domains = HashSet::new();
    let mut seen_urls = HashSet::new();
    let mut deduped = Vec::new();

    for r in results {
        if seen_urls.contains(&r.url) {
            continue;
        }
        seen_urls.insert(r.url.clone());

        if let Ok(u) = Url::parse(&r.url)
            && let Some(domain) = u.host_str()
        {
            if seen_domains.contains(domain) {
                continue;
            }
            seen_domains.insert(domain.to_string());
        }

        deduped.push(r);
    }
    deduped
}

fn format_search_results(results: &[SearchResult], limit: usize) -> String {
    let mut out = String::new();
    for (i, r) in results.iter().take(limit).enumerate() {
        let idx = i + 1;
        out.push_str(&format!("{idx}. {}\n   URL: {}\n", r.title, r.url));
        if !r.abstract_text.is_empty() {
            out.push_str(&format!("   Summary: {}\n", r.abstract_text));
        }
        out.push('\n');
    }
    out.trim_end().to_string()
}

impl Tool for WebSearchTool {
    const NAME: &'static str = "websearch";
    type Args = SearchArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Search the web and return structured search results with titles, summaries, and URLs.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<SearchArgs>()
    }

    async fn call(&self, _context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        into_rig_result(self.execute(args).await)
    }
}
