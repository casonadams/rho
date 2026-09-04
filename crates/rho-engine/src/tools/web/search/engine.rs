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
    match engine {
        EngineKind::Brave => brave::search_brave(req).await,
        EngineKind::DuckDuckGoLite => ddg_lite::search_ddg_lite(req).await,
        EngineKind::Yahoo => yahoo::search_yahoo(req).await,
        EngineKind::Firecrawl => firecrawl::search_firecrawl(req).await,
    }
}

pub async fn search_multi_engine(params: MultiEngineParams<'_>) -> Vec<SearchResult> {
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
    let req = EngineRequest {
        http: params.http,
        timeout_sec: params.timeout_sec,
        region: params.region,
        query: params.query,
        recency: params.recency,
    };

    for engine in engines {
        params.rate_limiter.acquire().await;
        let res = search_single_engine(engine, &req).await;

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
