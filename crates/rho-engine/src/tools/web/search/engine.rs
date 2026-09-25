use std::future::Future;
use std::pin::Pin;

use crate::tools::web::http::HttpClient;
use crate::tools::web::rate_limiter::SearchRateLimiter;
use crate::tools::web::search::query::{matches_domain_filters, normalize_domain_filters};
use crate::tools::web::search::result::{SearchResult, deduplicate_results};
use crate::tools::web::search::{brave, ddg_lite, exa, firecrawl, gemini, yahoo};
use rho_harness_core::args::WebSearchRecency;
use rho_harness_core::error::AppError;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineKind {
    Brave,
    DuckDuckGoLite,
    Yahoo,
    Firecrawl,
    Exa,
    Gemini,
}

impl EngineKind {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Brave => "Brave",
            Self::DuckDuckGoLite => "DuckDuckGo Lite",
            Self::Yahoo => "Yahoo",
            Self::Firecrawl => "Firecrawl",
            Self::Exa => "Exa",
            Self::Gemini => "Gemini",
        }
    }
}

impl std::fmt::Display for EngineKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Brave => write!(f, "brave"),
            Self::DuckDuckGoLite => write!(f, "duckduckgo"),
            Self::Yahoo => write!(f, "yahoo"),
            Self::Firecrawl => write!(f, "firecrawl"),
            Self::Exa => write!(f, "exa"),
            Self::Gemini => write!(f, "gemini"),
        }
    }
}

impl std::str::FromStr for EngineKind {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "brave" => Ok(Self::Brave),
            "duckduckgo" | "ddg" | "ddg_lite" | "duckduckgo_lite" | "duckduckgolite" => Ok(Self::DuckDuckGoLite),
            "yahoo" => Ok(Self::Yahoo),
            "firecrawl" => Ok(Self::Firecrawl),
            "exa" => Ok(Self::Exa),
            "gemini" | "google" => Ok(Self::Gemini),
            other => Err(AppError::Config(format!(
                "Unknown search engine '{other}'. Supported engines: brave, duckduckgo, yahoo, firecrawl, exa, gemini"
            ))),
        }
    }
}

pub fn default_engine_chain() -> Vec<EngineKind> {
    vec![EngineKind::Brave, EngineKind::DuckDuckGoLite, EngineKind::Yahoo]
}

pub fn resolve_engine_chain(default: &str, fallback: &[String]) -> Result<Vec<EngineKind>, AppError> {
    let mut engines = Vec::new();
    let default_engine: EngineKind = default.parse()?;
    engines.push(default_engine);

    for item in fallback {
        let engine: EngineKind = item.parse()?;
        if !engines.contains(&engine) {
            engines.push(engine);
        }
    }

    Ok(engines)
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
    pub engines: &'a [EngineKind],
}

pub async fn search_single_engine(
    engine: EngineKind,
    req: &EngineRequest<'_>,
    allowed: &[String],
    blocked: &[String],
) -> Result<Vec<SearchResult>, AppError> {
    dispatch_engine_call(engine, req, allowed, blocked).await
}

fn dispatch_engine_call<'a>(
    engine: EngineKind,
    req: &'a EngineRequest<'_>,
    allowed: &'a [String],
    blocked: &'a [String],
) -> Pin<Box<dyn Future<Output = Result<Vec<SearchResult>, AppError>> + Send + 'a>> {
    match engine {
        EngineKind::Brave => Box::pin(brave::search_brave(req)),
        EngineKind::DuckDuckGoLite => Box::pin(ddg_lite::search_ddg_lite(req)),
        EngineKind::Yahoo => Box::pin(yahoo::search_yahoo(req)),
        EngineKind::Firecrawl => Box::pin(firecrawl::search_firecrawl(req)),
        EngineKind::Exa => Box::pin(exa::search_exa(req, allowed, blocked)),
        EngineKind::Gemini => Box::pin(gemini::search_gemini(req)),
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
) -> Vec<SearchResult> {
    limiter.acquire().await;
    if let Ok(results) = search_single_engine(engine, req, allowed, blocked).await {
        results
            .into_iter()
            .map(|mut r| {
                if r.source.is_none() {
                    r.source = Some(engine.name().to_string());
                }
                r
            })
            .filter(|r| filter_result_by_domains(r, allowed, blocked))
            .collect()
    } else {
        Vec::new()
    }
}

pub async fn search_multi_engine(params: MultiEngineParams<'_>) -> Vec<SearchResult> {
    let (allowed, blocked) = normalize_domain_filters(params.domains);
    let req = EngineRequest {
        http: params.http,
        timeout_sec: params.timeout_sec,
        region: params.region,
        query: params.query,
        recency: params.recency,
    };

    search_multi_engine_impl(params.engines, params.limit, |engine| {
        query_engine_filtered(engine, &req, params.rate_limiter, &allowed, &blocked)
    })
    .await
}

async fn search_multi_engine_impl<F, Fut>(engines: &[EngineKind], limit: usize, mut query_fn: F) -> Vec<SearchResult>
where
    F: FnMut(EngineKind) -> Fut,
    Fut: std::future::Future<Output = Vec<SearchResult>>,
{
    for &engine in engines {
        let results = query_fn(engine).await;
        if !results.is_empty() {
            let deduped = deduplicate_results(results);
            return deduped.into_iter().take(limit).collect();
        }
    }

    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_kind_parsing() {
        let cases = [
            ("brave", EngineKind::Brave),
            ("duckduckgo", EngineKind::DuckDuckGoLite),
            ("ddg", EngineKind::DuckDuckGoLite),
            ("ddg_lite", EngineKind::DuckDuckGoLite),
            ("yahoo", EngineKind::Yahoo),
            ("firecrawl", EngineKind::Firecrawl),
            ("exa", EngineKind::Exa),
            ("gemini", EngineKind::Gemini),
            ("google", EngineKind::Gemini),
        ];
        for (input, expected) in cases {
            assert_eq!(input.parse::<EngineKind>().unwrap(), expected);
        }
        assert!("unknown".parse::<EngineKind>().is_err());
    }

    #[test]
    fn test_engine_kind_display() {
        let cases = [
            (EngineKind::Brave, "brave"),
            (EngineKind::DuckDuckGoLite, "duckduckgo"),
            (EngineKind::Yahoo, "yahoo"),
            (EngineKind::Firecrawl, "firecrawl"),
            (EngineKind::Exa, "exa"),
            (EngineKind::Gemini, "gemini"),
        ];
        for (engine, expected) in cases {
            assert_eq!(engine.to_string(), expected);
        }
    }

    #[test]
    fn test_engine_kind_name() {
        let cases = [
            (EngineKind::Brave, "Brave"),
            (EngineKind::DuckDuckGoLite, "DuckDuckGo Lite"),
            (EngineKind::Yahoo, "Yahoo"),
            (EngineKind::Firecrawl, "Firecrawl"),
            (EngineKind::Exa, "Exa"),
            (EngineKind::Gemini, "Gemini"),
        ];
        for (engine, expected) in cases {
            assert_eq!(engine.name(), expected);
        }
    }

    #[tokio::test]
    async fn test_dispatch_engine_call_all_variants() {
        let http = HttpClient::new(true).unwrap();
        let req = EngineRequest {
            http: &http,
            timeout_sec: 1,
            region: "us-en",
            query: "test",
            recency: None,
        };
        let allowed = vec![];
        let blocked = vec![];

        for engine in [
            EngineKind::Brave,
            EngineKind::DuckDuckGoLite,
            EngineKind::Yahoo,
            EngineKind::Firecrawl,
            EngineKind::Exa,
            EngineKind::Gemini,
        ] {
            let fut = dispatch_engine_call(engine, &req, &allowed, &blocked);
            drop(fut);
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

        let results = search_multi_engine_impl(&engines, 5, move |engine| {
            let called = called_clone.clone();
            async move {
                called.lock().unwrap().push(engine);
                match engine {
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
                }
            }
        })
        .await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Brave Result");
        assert_eq!(*called.lock().unwrap(), vec![EngineKind::Brave]);
    }

    #[tokio::test]
    async fn test_search_multi_engine_fallback_on_first_failure() {
        let called = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let called_clone = called.clone();
        let engines = [EngineKind::Brave, EngineKind::DuckDuckGoLite, EngineKind::Yahoo];

        let results = search_multi_engine_impl(&engines, 5, move |engine| {
            let called = called_clone.clone();
            async move {
                called.lock().unwrap().push(engine);
                match engine {
                    EngineKind::Brave => Vec::new(),
                    EngineKind::DuckDuckGoLite => {
                        vec![SearchResult::new("DDG Result", "Content", "https://example.com/ddg")]
                    }
                    _ => vec![SearchResult::new(
                        "Yahoo Result",
                        "Content",
                        "https://example.com/yahoo",
                    )],
                }
            }
        })
        .await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "DDG Result");
        assert_eq!(
            *called.lock().unwrap(),
            vec![EngineKind::Brave, EngineKind::DuckDuckGoLite]
        );
    }

    #[tokio::test]
    async fn test_search_multi_engine_single_engine_no_fallback() {
        let called = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let called_clone = called.clone();
        let engines = [EngineKind::Brave];

        let results = search_multi_engine_impl(&engines, 5, move |engine| {
            let called = called_clone.clone();
            async move {
                called.lock().unwrap().push(engine);
                Vec::new()
            }
        })
        .await;

        assert!(results.is_empty());
        assert_eq!(*called.lock().unwrap(), vec![EngineKind::Brave]);
    }

    #[tokio::test]
    async fn test_search_multi_engine_truncates_to_limit() {
        let engines = [EngineKind::Brave];
        let results = search_multi_engine_impl(&engines, 2, |_engine| async move {
            vec![
                SearchResult::new("1", "c", "https://example.com/1"),
                SearchResult::new("2", "c", "https://example.com/2"),
                SearchResult::new("3", "c", "https://example.com/3"),
            ]
        })
        .await;

        assert_eq!(results.len(), 2);
    }
}
