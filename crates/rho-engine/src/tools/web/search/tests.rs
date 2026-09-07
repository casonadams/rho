use super::*;

#[test]
fn test_normalize_domain() {
    let cases = [
        ("https://www.github.com/path", Some("github.com".to_string())),
        ("http://docs.rs:443", Some("docs.rs".to_string())),
        ("-www.bad-site.org/", Some("bad-site.org".to_string())),
        ("invalid domain!", None),
        ("", None),
    ];
    for (input, expected) in cases {
        assert_eq!(normalize_domain(input), expected);
    }
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

#[test]
fn test_matches_domain_filters() {
    let allowed = vec!["github.com".to_string(), "docs.rs".to_string()];
    let blocked = vec!["blog.github.com".to_string(), "spam.com".to_string()];
    let cases = [
        ("github.com", true),
        ("raw.github.com", true),
        ("blog.github.com", false),
        ("spam.com", false),
        ("other.org", false),
    ];
    for (domain, expected) in cases {
        assert_eq!(matches_domain_filters(domain, &allowed, &blocked), expected);
    }
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
        SearchResult::new("Docs", "API Docs", "https://docs.rs/tokio")
            .with_hint("documentation")
            .with_source("Brave"),
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
