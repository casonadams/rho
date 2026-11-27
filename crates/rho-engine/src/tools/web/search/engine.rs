use std::path::Path;

use crate::tools::web::http::{HttpClient, HttpRequest};
use crate::tools::web::rate_limiter::SearchRateLimiter;
use crate::tools::web::search::query::{
    build_search_query_with_filters, matches_domain_filters, normalize_domain_filters,
};
use crate::tools::web::search::result::{SearchResult, deduplicate_results};
use crate::tools::web::search::{brave, ddg_lite, exa, yahoo};
use rho_harness_core::args::WebSearchRecency;
use rho_harness_core::config::SearchEngine;
use rho_harness_core::error::AppError;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineKind {
    Brave,
    DuckDuckGoLite,
    Yahoo,
    Exa,
}

impl EngineKind {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Brave => "Brave",
            Self::DuckDuckGoLite => "DuckDuckGo Lite",
            Self::Yahoo => "Yahoo",
            Self::Exa => "Exa",
        }
    }
}

impl From<SearchEngine> for EngineKind {
    fn from(engine: SearchEngine) -> Self {
        match engine {
            SearchEngine::Brave => Self::Brave,
            SearchEngine::DuckDuckGo => Self::DuckDuckGoLite,
            SearchEngine::Yahoo => Self::Yahoo,
            SearchEngine::Exa => Self::Exa,
        }
    }
}

pub fn default_engine_chain() -> Vec<EngineKind> {
    vec![EngineKind::Brave, EngineKind::DuckDuckGoLite, EngineKind::Yahoo]
}

pub fn resolve_engine_chain(default: &str, fallback: &[String]) -> Result<Vec<EngineKind>, AppError> {
    let mut engines = vec![parse_engine(default)?];
    for item in fallback {
        let engine = parse_engine(item)?;
        if !engines.contains(&engine) {
            engines.push(engine);
        }
    }
    Ok(engines)
}

fn parse_engine(name: &str) -> Result<EngineKind, AppError> {
    let engine: SearchEngine = name.parse().map_err(AppError::Config)?;
    Ok(engine.into())
}

pub struct EngineRequest<'a> {
    pub http: &'a HttpClient,
    pub timeout_sec: u64,
    pub region: &'a str,
    pub query: &'a str,
    pub limit: usize,
    pub recency: Option<WebSearchRecency>,
    pub allowed_domains: &'a [String],
    pub blocked_domains: &'a [String],
    pub auth_file: Option<&'a Path>,
}

pub async fn fetch_results_page(req: &EngineRequest<'_>, url: &str, user_agent: &str) -> Result<String, AppError> {
    let resp = req
        .http
        .get_text(HttpRequest {
            url,
            user_agent: Some(user_agent),
            timeout_sec: req.timeout_sec,
            max_bytes: 2_000_000,
            pdf_max_bytes: None,
        })
        .await?;
    Ok(resp.body)
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
    pub engines: &'a [EngineKind],
    pub auth_file: Option<&'a Path>,
}

async fn dispatch_engine_call(engine: EngineKind, req: &EngineRequest<'_>) -> Result<Vec<SearchResult>, AppError> {
    match engine {
        EngineKind::Brave => brave::search_brave(req).await,
        EngineKind::DuckDuckGoLite => ddg_lite::search_ddg_lite(req).await,
        EngineKind::Yahoo => yahoo::search_yahoo(req).await,
        EngineKind::Exa => exa::search_exa(req).await,
    }
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
    req: &EngineRequest<'_>,
    limiter: &SearchRateLimiter,
    allowed: &[String],
    blocked: &[String],
) -> Result<Vec<SearchResult>, AppError> {
    limiter.acquire().await;
    let results = dispatch_engine_call(engine, req).await?;
    Ok(results
        .into_iter()
        .map(|mut r| {
            if r.source.is_none() {
                r.source = Some(engine.name().to_string());
            }
            r
        })
        .filter(|r| filter_result_by_domains(r, allowed, blocked))
        .collect())
}

pub struct MultiEngineOutcome {
    pub results: Vec<SearchResult>,
    pub last_error: Option<AppError>,
}

pub async fn search_multi_engine(params: MultiEngineParams<'_>) -> Vec<SearchResult> {
    search_multi_engine_outcome(params).await.results
}

pub(crate) async fn search_multi_engine_outcome(params: MultiEngineParams<'_>) -> MultiEngineOutcome {
    let (allowed, blocked) = normalize_domain_filters(params.domains);
    let scraper_query = build_search_query_with_filters(params.query, params.domains);
    let clean_query = params.query.split_whitespace().collect::<Vec<_>>().join(" ");

    search_multi_engine_impl(params.engines, params.limit, |engine| {
        let query = match engine {
            EngineKind::Exa => clean_query.as_str(),
            EngineKind::Brave | EngineKind::DuckDuckGoLite | EngineKind::Yahoo => scraper_query.as_str(),
        };
        let req = EngineRequest {
            http: params.http,
            timeout_sec: params.timeout_sec,
            region: params.region,
            query,
            limit: params.limit,
            recency: params.recency,
            allowed_domains: &allowed,
            blocked_domains: &blocked,
            auth_file: params.auth_file,
        };
        let allowed = &allowed;
        let blocked = &blocked;
        let rate_limiter = params.rate_limiter;
        async move { query_engine_filtered(engine, &req, rate_limiter, allowed, blocked).await }
    })
    .await
}

async fn search_multi_engine_impl<F, Fut>(engines: &[EngineKind], limit: usize, mut query_fn: F) -> MultiEngineOutcome
where
    F: FnMut(EngineKind) -> Fut,
    Fut: Future<Output = Result<Vec<SearchResult>, AppError>>,
{
    let mut last_error: Option<AppError> = None;
    for &engine in engines {
        match query_fn(engine).await {
            Ok(results) if !results.is_empty() => {
                let deduped = deduplicate_results(results);
                return MultiEngineOutcome {
                    results: deduped.into_iter().take(limit).collect(),
                    last_error: None,
                };
            }
            Ok(_) => {}
            Err(err) => {
                last_error = Some(err);
            }
        }
    }

    MultiEngineOutcome {
        results: Vec::new(),
        last_error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_kind_parsing() {
        for (input, expected) in [
            ("brave", EngineKind::Brave),
            ("duckduckgo", EngineKind::DuckDuckGoLite),
            ("ddg", EngineKind::DuckDuckGoLite),
            ("ddg_lite", EngineKind::DuckDuckGoLite),
            ("yahoo", EngineKind::Yahoo),
            ("exa", EngineKind::Exa),
            ("EXA", EngineKind::Exa),
        ] {
            assert_eq!(resolve_engine_chain(input, &[]).unwrap(), vec![expected]);
        }
        assert!(resolve_engine_chain("unknown", &[]).is_err());
        assert!(resolve_engine_chain("brave", &["gemini".to_string()]).is_err());
    }

    #[test]
    fn test_engine_kind_name() {
        let cases = [
            (EngineKind::Brave, "Brave"),
            (EngineKind::DuckDuckGoLite, "DuckDuckGo Lite"),
            (EngineKind::Yahoo, "Yahoo"),
            (EngineKind::Exa, "Exa"),
        ];
        for (engine, name) in cases {
            assert_eq!(engine.name(), name);
        }
    }

    #[test]
    fn test_resolve_engine_chain_deduplication() {
        let chain = resolve_engine_chain(
            "brave",
            &["duckduckgo".to_string(), "brave".to_string(), "yahoo".to_string()],
        )
        .unwrap();
        assert_eq!(
            chain,
            vec![EngineKind::Brave, EngineKind::DuckDuckGoLite, EngineKind::Yahoo,]
        );

        let exa_chain = resolve_engine_chain("exa", &["brave".to_string(), "exa".to_string()]).unwrap();
        assert_eq!(exa_chain, vec![EngineKind::Exa, EngineKind::Brave]);
    }

    #[test]
    fn test_default_engine_chain() {
        assert_eq!(
            default_engine_chain(),
            vec![EngineKind::Brave, EngineKind::DuckDuckGoLite, EngineKind::Yahoo,]
        );
    }

    #[tokio::test]
    async fn test_search_multi_engine_first_engine_succeeds_without_fallback() {
        let called = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let called_clone = called.clone();
        let engines = [EngineKind::Brave, EngineKind::DuckDuckGoLite, EngineKind::Yahoo];

        let outcome = search_multi_engine_impl(&engines, 5, move |engine| {
            let called = called_clone.clone();
            async move {
                called.lock().unwrap().push(engine);
                Ok(match engine {
                    EngineKind::Brave => vec![SearchResult::new(
                        "Brave Result",
                        "Content",
                        "https://example.com/brave",
                    )],
                    _ => vec![SearchResult::new(
                        "Other Result",
                        "Content",
                        "https://example.com/other",
                    )],
                })
            }
        })
        .await;

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].title, "Brave Result");
        assert!(outcome.last_error.is_none());
        assert_eq!(*called.lock().unwrap(), vec![EngineKind::Brave]);
    }

    #[tokio::test]
    async fn test_search_multi_engine_fallback_on_error_then_empty_then_success() {
        let called = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let called_clone = called.clone();
        let engines = [EngineKind::Brave, EngineKind::DuckDuckGoLite, EngineKind::Yahoo];

        let outcome = search_multi_engine_impl(&engines, 5, move |engine| {
            let called = called_clone.clone();
            async move {
                called.lock().unwrap().push(engine);
                match engine {
                    EngineKind::Brave => Err(AppError::Tool("timeout".to_string())),
                    EngineKind::DuckDuckGoLite => Ok(Vec::new()),
                    _ => Ok(vec![SearchResult::new(
                        "Yahoo Result",
                        "Content",
                        "https://example.com/yahoo",
                    )]),
                }
            }
        })
        .await;

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].title, "Yahoo Result");
        assert!(outcome.last_error.is_none());
        assert_eq!(
            *called.lock().unwrap(),
            vec![EngineKind::Brave, EngineKind::DuckDuckGoLite, EngineKind::Yahoo]
        );
    }

    #[tokio::test]
    async fn test_search_multi_engine_single_engine_no_fallback() {
        let called = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let called_clone = called.clone();
        let engines = [EngineKind::Brave];

        let outcome = search_multi_engine_impl(&engines, 5, move |engine| {
            let called = called_clone.clone();
            async move {
                called.lock().unwrap().push(engine);
                Ok(Vec::new())
            }
        })
        .await;

        assert!(outcome.results.is_empty());
        assert!(outcome.last_error.is_none());
        assert_eq!(*called.lock().unwrap(), vec![EngineKind::Brave]);
    }

    #[tokio::test]
    async fn test_search_multi_engine_truncates_to_limit() {
        let engines = [EngineKind::Brave];
        let outcome = search_multi_engine_impl(&engines, 2, |_engine| async move {
            Ok(vec![
                SearchResult::new("1", "c", "https://example.com/1"),
                SearchResult::new("2", "c", "https://example.com/2"),
                SearchResult::new("3", "c", "https://example.com/3"),
            ])
        })
        .await;

        assert_eq!(outcome.results.len(), 2);
    }
}
