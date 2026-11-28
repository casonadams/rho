use super::query::{
    build_search_query_with_filters, matches_domain_filters, normalize_domain, normalize_domain_filters,
};
use super::result::{SearchResult, deduplicate_results};
use super::*;
use test_case::test_case;

#[test_case("https://www.github.com/path", Some("github.com"); "url with www and path")]
#[test_case("http://docs.rs:443", Some("docs.rs"); "url with port")]
#[test_case("-www.bad-site.org/", Some("bad-site.org"); "blocked prefix")]
#[test_case("invalid domain!", None; "invalid")]
#[test_case("", None; "empty")]
fn test_normalize_domain(input: &str, expected: Option<&str>) {
    assert_eq!(normalize_domain(input).as_deref(), expected);
}

#[test]
fn test_normalize_domain_filters() {
    let domains = vec![
        "github.com".to_string(),
        "-spam.com".to_string(),
        "https://docs.rs".to_string(),
        "-https://www.bad.org/page".to_string(),
    ];
    let (allowed, blocked) = normalize_domain_filters(Some(&domains));
    assert_eq!(allowed, vec!["github.com", "docs.rs"]);
    assert_eq!(blocked, vec!["spam.com", "bad.org"]);
}

#[test_case("github.com", true; "allowed")]
#[test_case("raw.github.com", true; "allowed subdomain")]
#[test_case("blog.github.com", false; "blocked subdomain")]
#[test_case("spam.com", false; "blocked")]
#[test_case("other.org", false; "not allowed")]
fn test_matches_domain_filters(domain: &str, expected: bool) {
    let allowed = vec!["github.com".to_string(), "docs.rs".to_string()];
    let blocked = vec!["blog.github.com".to_string(), "spam.com".to_string()];
    assert_eq!(matches_domain_filters(domain, &allowed, &blocked), expected);
}

#[test]
fn test_build_search_query_with_filters() {
    let domains = vec!["vitest.dev".to_string(), "-spam.com".to_string()];
    assert_eq!(
        build_search_query_with_filters("vitest documentation", Some(&domains)),
        "vitest documentation site:vitest.dev -site:spam.com"
    );

    let multi_domains = vec!["a.com".to_string(), "b.com".to_string()];
    assert_eq!(
        build_search_query_with_filters("multi", Some(&multi_domains)),
        "multi site:a.com OR site:b.com"
    );
}

#[test]
fn test_relax_query() {
    assert_eq!(
        relax_query("\"exact match\" +term (group) [tag]"),
        "exact match term group tag"
    );
}

#[test]
fn test_deduplicate_results_canonical_url() {
    let results = vec![
        SearchResult::new("Doc 1", "first", "https://docs.rs/crate/a?utm_source=twitter"),
        SearchResult::new("Doc 2", "second page", "https://docs.rs/crate/b"),
        SearchResult::new("Exact URL Dup", "dup", "https://docs.rs/crate/a"),
        SearchResult::new(
            "Repo Blob",
            "rust repo",
            "https://github.com/rust-lang/rust/blob/main/README.md",
        ),
    ];
    let deduped = deduplicate_results(results);
    let expected = [
        ("https://docs.rs/crate/a", Some("documentation")),
        ("https://docs.rs/crate/b", Some("documentation")),
        (
            "https://raw.githubusercontent.com/rust-lang/rust/main/README.md",
            Some("GitHub"),
        ),
    ];
    assert_eq!(deduped.len(), expected.len());
    for (res, (url, hint)) in deduped.iter().zip(expected) {
        assert_eq!(res.url, url);
        assert_eq!(res.content_hint.as_deref(), hint);
    }
}

#[test]
fn test_format_search_results() {
    let results = vec![
        SearchResult::new("Rust", "A systems language", "https://www.rust-lang.org/"),
        SearchResult {
            content_hint: Some("documentation".into()),
            source: Some("Brave".into()),
            ..SearchResult::new("Docs", "API Docs", "https://docs.rs/tokio")
        },
    ];
    let formatted = format_search_results(FormatResultsParams {
        query: "rust lang",
        results: &results,
        limit: 2,
        today: "2026-09-03",
    });
    assert!(formatted.contains("via Brave"));
    assert!(formatted.contains("1. **Rust** (rust-lang.org)\n   URL: https://www.rust-lang.org/"));
    assert!(formatted.contains("2. **Docs** (docs.rs | documentation | Brave)\n   URL: https://docs.rs/tokio"));
}

#[tokio::test]
async fn perform_search_without_configured_engines_returns_no_results() {
    let tool = WebSearchTool {
        http: HttpClient::new(false).unwrap(),
        rate_limiter: SearchRateLimiter::new(0),
        config: WebSearchConfig {
            region: "us-en".to_string(),
            timeout_sec: 5,
            engines: Vec::new(),
            auth_file: None,
        },
    };
    let outcome = tool.perform_search("rust", 5, None, None).await;
    assert!(outcome.results.is_empty());
    assert!(outcome.last_error.is_none());
}

fn fake_engine_outcome(results: Vec<SearchResult>, last_error: Option<AppError>) -> engine::MultiEngineOutcome {
    engine::MultiEngineOutcome { results, last_error }
}

#[tokio::test]
async fn relaxed_results_are_labelled_with_relaxed_query() {
    let queries = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = queries.clone();
    let result = try_relaxed_search("rust +async runtime", 5, "2026-11-27", move |relaxed: String| {
        let recorded = recorded.clone();
        async move {
            recorded.lock().unwrap().push(relaxed);
            fake_engine_outcome(vec![SearchResult::new("Tokio", "runtime", "https://tokio.rs/")], None)
        }
    })
    .await
    .unwrap()
    .expect("relaxed search should produce a result");

    assert!(!result.is_error);
    assert_eq!(*queries.lock().unwrap(), vec!["rust async runtime".to_string()]);
    assert!(result.content.starts_with(
        "No search results found for: \"rust +async runtime\". Results for relaxed query \"rust async runtime\":"
    ));
}

#[tokio::test]
async fn relaxed_attempt_error_is_reported() {
    let error = try_relaxed_search("rust +async runtime", 5, "2026-11-27", |_relaxed: String| async {
        fake_engine_outcome(Vec::new(), Some(AppError::Tool("timeout".to_string())))
    })
    .await
    .unwrap_err();

    assert!(error.to_string().contains("timeout"));
}

fn failed(why: &str) -> AppError {
    AppError::Tool(why.to_string())
}

#[test_case(Ok(Some(ToolResult::success("relaxed hits"))), Some(failed("first")), "relaxed hits", false; "relaxed hits win")]
#[test_case(Err(failed("retry")), Some(failed("first")), "first", true; "first error before the retry")]
#[test_case(Err(failed("retry")), None, "retry", true; "retry error alone")]
#[test_case(Ok(None), None, "No search results found", false; "nothing found")]
fn an_empty_search_reports_relaxed_results_then_the_first_error(
    relaxed: Result<Option<ToolResult>, AppError>,
    last_error: Option<AppError>,
    expected: &str,
    is_error: bool,
) {
    let result = empty_search_result("q", "2026-11-27", relaxed, last_error);
    assert!(result.content.contains(expected), "{}", result.content);
    assert_eq!(result.is_error, is_error);
}

#[tokio::test]
async fn unchanged_query_skips_relaxed_attempt() {
    let called = std::sync::Arc::new(std::sync::Mutex::new(false));
    let flag = called.clone();
    let result = try_relaxed_search("rust", 5, "2026-11-27", move |_relaxed: String| {
        *flag.lock().unwrap() = true;
        async { fake_engine_outcome(Vec::new(), None) }
    })
    .await;

    assert!(matches!(result, Ok(None)));
    assert!(!*called.lock().unwrap());
}
