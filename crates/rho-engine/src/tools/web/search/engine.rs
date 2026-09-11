use crate::tools::web::http::HttpClient;
use crate::tools::web::rate_limiter::SearchRateLimiter;
use crate::tools::web::search::query::{matches_domain_filters, normalize_domain_filters};
use crate::tools::web::search::result::{SearchResult, deduplicate_results};
use crate::tools::web::search::{brave, ddg_lite, firecrawl, yahoo};
use rand::seq::SliceRandom;
use rho_harness_core::args::WebSearchRecency;
use rho_harness_core::error::AppError;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineKind {
    Brave,
    DuckDuckGoLite,
    Yahoo,
    Firecrawl,
}

impl EngineKind {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Brave => "Brave",
            Self::DuckDuckGoLite => "DuckDuckGo Lite",
            Self::Yahoo => "Yahoo",
            Self::Firecrawl => "Firecrawl",
        }
    }
}

pub struct EngineRequest<'a> {
    pub http: &'a HttpClient,
    pub timeout_sec: u64,
    pub region: &'a str,
    pub query: &'a str,
    pub recency: Option<WebSearchRecency>,
}

pub struct MultiEngineParams<'a> {
    pub http: &'a HttpClient,
    pub rate_limiter: &'a SearchRateLimiter,
    pub region: &'a str,
    pub timeout_sec: u64,
    pub query: &'a str,
    pub limit: usize,
    pub recency: Option<WebSearchRecency>,
    pub domains: Option<&'a [String]>,
}

pub async fn search_single_engine(engine: EngineKind, req: &EngineRequest<'_>) -> Result<Vec<SearchResult>, AppError> {
    dispatch_engine_call(engine, req).await
}

fn dispatch_engine_call<'a>(
    engine: EngineKind,
    req: &'a EngineRequest<'_>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<SearchResult>, AppError>> + Send + 'a>> {
    match engine {
        EngineKind::Brave => Box::pin(brave::search_brave(req)),
        EngineKind::DuckDuckGoLite => Box::pin(ddg_lite::search_ddg_lite(req)),
        EngineKind::Yahoo => Box::pin(yahoo::search_yahoo(req)),
        EngineKind::Firecrawl => Box::pin(firecrawl::search_firecrawl(req)),
    }
}

fn shuffled_engines() -> Vec<EngineKind> {
    let mut list = vec![
        EngineKind::Brave,
        EngineKind::DuckDuckGoLite,
        EngineKind::Yahoo,
        EngineKind::Firecrawl,
    ];
    list.shuffle(&mut rand::rng());
    list
}

fn filter_result_by_domains(r: &SearchResult, allowed: &[String], blocked: &[String]) -> bool {
    let Ok(u) = Url::parse(&r.url) else {
        return false;
    };
    u.host_str()
        .is_some_and(|host| matches_domain_filters(host, allowed, blocked))
}

async fn query_engine_filtered(
    engine: EngineKind,
    (req, limiter): (&EngineRequest<'_>, &SearchRateLimiter),
    filters: (&[String], &[String]),
) -> Vec<SearchResult> {
    limiter.acquire().await;
    if let Ok(results) = search_single_engine(engine, req).await {
        results
            .into_iter()
            .map(|mut r| {
                if r.source.is_none() {
                    r.source = Some(engine.name().to_string());
                }
                r
            })
            .filter(|r| filter_result_by_domains(r, filters.0, filters.1))
            .collect()
    } else {
        Vec::new()
    }
}

pub async fn search_multi_engine(params: MultiEngineParams<'_>) -> Vec<SearchResult> {
    let engines = shuffled_engines();
    let (allowed, blocked) = normalize_domain_filters(params.domains);
    let req = EngineRequest {
        http: params.http,
        timeout_sec: params.timeout_sec,
        region: params.region,
        query: params.query,
        recency: params.recency,
    };

    let mut accumulated: Vec<SearchResult> = Vec::new();
    for engine in engines {
        let results = query_engine_filtered(engine, (&req, params.rate_limiter), (&allowed, &blocked)).await;
        accumulated.extend(results);
        let deduped = deduplicate_results(accumulated);
        if deduped.len() >= params.limit {
            return deduped.into_iter().take(params.limit).collect();
        }
        accumulated = deduped;
    }

    accumulated.into_iter().take(params.limit).collect()
}
