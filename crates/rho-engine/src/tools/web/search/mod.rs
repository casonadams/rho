pub mod brave;
pub mod ddg_lite;
pub mod firecrawl;
pub mod result;
pub mod yahoo;

#[cfg(test)]
mod tests;

use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
use crate::tools::web::http::{BRAVE_CHROME_UA, HttpClient, HttpRequest, LYNX_UA};
use crate::tools::web::rate_limiter::SearchRateLimiter;
use rand::seq::SliceRandom;
use result::SearchResult;
pub use rho_harness_core::args::{SearchArgs, SearchRecency};
use rho_harness_core::error::AppError;
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

pub struct SearchQueryParams<'a> {
    pub query: &'a str,
    pub limit: usize,
    pub recency: Option<SearchRecency>,
    pub domains: Option<&'a [String]>,
}

pub struct FormatResultsParams<'a> {
    pub query: &'a str,
    pub results: &'a [SearchResult],
    pub limit: usize,
    pub today: &'a str,
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
        let domains = args.domains.as_deref();
        let recency = args.recency;
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();

        let effective_query = build_search_query_with_filters(query, domains);

        // Perform rate-limited multi-engine search
        let results = self
            .search_multi_engine(SearchQueryParams {
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
                        .search_multi_engine(SearchQueryParams {
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

    async fn search_multi_engine(&self, params: SearchQueryParams<'_>) -> Vec<SearchResult> {
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

        let (allowed, blocked) = normalize_domain_filters(params.domains);

        for engine in engines {
            self.rate_limiter.acquire().await;
            let res = match engine {
                EngineKind::Brave => self.search_brave(params.query, params.recency).await,
                EngineKind::DuckDuckGoLite => self.search_ddg_lite(params.query, params.recency).await,
                EngineKind::Yahoo => self.search_yahoo(params.query, params.recency).await,
                EngineKind::Firecrawl => self.search_firecrawl(params.query).await,
            };

            if let Ok(results) = res {
                let filtered: Vec<SearchResult> = results
                    .into_iter()
                    .filter(|r| match Url::parse(&r.url) {
                        Ok(u) => {
                            if let Some(host) = u.host_str() {
                                matches_domain_filters(host, &allowed, &blocked)
                            } else {
                                false
                            }
                        }
                        Err(_) => false,
                    })
                    .collect();

                let deduplicated = deduplicate_results(filtered);
                if !deduplicated.is_empty() {
                    return deduplicated.into_iter().take(params.limit).collect();
                }
            }
        }

        Vec::new()
    }

    async fn search_brave(&self, query: &str, recency: Option<SearchRecency>) -> Result<Vec<SearchResult>, AppError> {
        let tf_param = match recency {
            Some(SearchRecency::Day) => "&tf=pd",
            Some(SearchRecency::Week) => "&tf=pw",
            Some(SearchRecency::Month) => "&tf=pm",
            Some(SearchRecency::Year) => "&tf=py",
            None => "",
        };
        let url = format!(
            "https://search.brave.com/search?q={}&source=web{tf_param}",
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

    async fn search_ddg_lite(
        &self,
        query: &str,
        recency: Option<SearchRecency>,
    ) -> Result<Vec<SearchResult>, AppError> {
        let df_param = match recency {
            Some(SearchRecency::Day) => "&df=d",
            Some(SearchRecency::Week) => "&df=w",
            Some(SearchRecency::Month) => "&df=m",
            Some(SearchRecency::Year) => "&df=y",
            None => "",
        };
        let url = format!(
            "https://lite.duckduckgo.com/lite/?q={}&kl={}{df_param}",
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

    async fn search_yahoo(&self, query: &str, recency: Option<SearchRecency>) -> Result<Vec<SearchResult>, AppError> {
        let age_param = match recency {
            Some(SearchRecency::Day) => "&age=1d",
            Some(SearchRecency::Week) => "&age=1w",
            Some(SearchRecency::Month) => "&age=1m",
            Some(SearchRecency::Year) => "&age=1y",
            None => "",
        };
        let url = format!(
            "https://search.yahoo.com/search?p={}{age_param}",
            urlencoding_encode(query)
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

pub fn normalize_domain(raw: &str) -> Option<String> {
    let mut input = raw.trim().to_lowercase();
    if input.is_empty() {
        return None;
    }
    if let Some(stripped) = input.strip_prefix('-') {
        input = stripped.trim().to_string();
    }
    if input.is_empty() {
        return None;
    }
    if let Ok(parsed) = Url::parse(&input) {
        if let Some(host) = parsed.host_str() {
            input = host.to_string();
        }
    } else if let Ok(parsed) = Url::parse(&format!("https://{input}")) {
        if let Some(host) = parsed.host_str() {
            input = host.to_string();
        }
    } else {
        input = input.split('/').next()?.split(':').next()?.to_string();
    }
    let trimmed = input.trim_start_matches("www.").trim_matches('.').to_string();
    if trimmed.contains('.') && !trimmed.contains(' ') {
        Some(trimmed)
    } else {
        None
    }
}

pub fn normalize_domain_filters(domains: Option<&[String]>) -> (Vec<String>, Vec<String>) {
    let mut allowed = Vec::new();
    let mut blocked = Vec::new();
    let Some(domains) = domains else {
        return (allowed, blocked);
    };

    for raw in domains {
        let is_blocked = raw.trim().starts_with('-');
        if let Some(domain) = normalize_domain(raw) {
            if is_blocked {
                if !blocked.contains(&domain) {
                    blocked.push(domain);
                }
            } else if !allowed.contains(&domain) {
                allowed.push(domain);
            }
        }
    }
    (allowed, blocked)
}

fn matches_site(host: &str, target_domain: &str) -> bool {
    let normalized_host = host.strip_prefix("www.").unwrap_or(host).to_lowercase();
    let normalized_target = target_domain
        .strip_prefix("www.")
        .unwrap_or(target_domain)
        .to_lowercase();
    normalized_host == normalized_target || normalized_host.ends_with(&format!(".{normalized_target}"))
}

pub fn matches_domain_filters(host: &str, allowed: &[String], blocked: &[String]) -> bool {
    if allowed.is_empty() && blocked.is_empty() {
        return true;
    }
    if !allowed.is_empty() && !allowed.iter().any(|domain| matches_site(host, domain)) {
        return false;
    }
    !blocked.iter().any(|domain| matches_site(host, domain))
}

pub fn build_search_query_with_filters(query: &str, domains: Option<&[String]>) -> String {
    let cleaned = query.split_whitespace().collect::<Vec<_>>().join(" ");
    let (allowed, blocked) = normalize_domain_filters(domains);
    if allowed.is_empty() && blocked.is_empty() {
        return cleaned;
    }

    let mut parts = vec![cleaned];
    if allowed.len() == 1 && !parts[0].to_lowercase().contains("site:") {
        parts.push(format!("site:{}", allowed[0]));
    } else if allowed.len() > 1 && !parts[0].to_lowercase().contains("site:") {
        let sites = allowed
            .iter()
            .map(|d| format!("site:{d}"))
            .collect::<Vec<_>>()
            .join(" OR ");
        parts.push(sites);
    }

    for b in blocked {
        let neg = format!("-site:{b}");
        if !parts[0].contains(&neg) {
            parts.push(neg);
        }
    }

    parts.join(" ").trim().to_string()
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

fn format_search_results(params: FormatResultsParams<'_>) -> String {
    let mut out = format!(
        "**Search results for:** {} (searched on {})\n\n",
        params.query, params.today
    );
    for (i, r) in params.results.iter().take(params.limit).enumerate() {
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
    const NAME: &'static str = "search";
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
