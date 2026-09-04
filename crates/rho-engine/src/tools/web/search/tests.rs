use super::*;

#[test]
fn test_normalize_domain() {
    assert_eq!(
        normalize_domain("https://www.github.com/path"),
        Some("github.com".to_string())
    );
    assert_eq!(normalize_domain("http://docs.rs:443"), Some("docs.rs".to_string()));
    assert_eq!(normalize_domain("-www.bad-site.org/"), Some("bad-site.org".to_string()));
    assert_eq!(normalize_domain("invalid domain!"), None);
    assert_eq!(normalize_domain(""), None);
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

    assert!(matches_domain_filters("github.com", &allowed, &blocked));
    assert!(matches_domain_filters("raw.github.com", &allowed, &blocked));
    assert!(!matches_domain_filters("blog.github.com", &allowed, &blocked));
    assert!(!matches_domain_filters("spam.com", &allowed, &blocked));
    assert!(!matches_domain_filters("other.org", &allowed, &blocked));
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
fn test_deduplicate_results() {
    let results = vec![
        SearchResult::new("Doc 1", "first", "https://docs.rs/crate/a"),
        SearchResult::new("Doc 2", "duplicate domain", "https://docs.rs/crate/b"),
        SearchResult::new("Exact URL Dup", "dup", "https://docs.rs/crate/a"),
        SearchResult::new("Repo", "rust repo", "https://github.com/rust-lang/rust"),
    ];
    let deduped = deduplicate_results(results);
    assert_eq!(deduped.len(), 2);
    assert_eq!(deduped[0].url, "https://docs.rs/crate/a");
    assert_eq!(deduped[1].url, "https://github.com/rust-lang/rust");
}

#[test]
fn test_format_search_results() {
    let results = vec![
        SearchResult::new("Rust", "A systems programming language", "https://www.rust-lang.org/"),
        SearchResult::new("Crates.io", "Registry", "https://crates.io/"),
    ];
    let formatted = format_search_results(FormatResultsParams {
        query: "rust lang",
        results: &results,
        limit: 1,
        today: "2026-09-03",
    });
    assert!(formatted.starts_with("**Search results for:** rust lang (searched on 2026-09-03)"));
    assert!(formatted.contains("1. Rust"));
    assert!(formatted.contains("URL: https://www.rust-lang.org/"));
    assert!(formatted.contains("Summary: A systems programming language"));
    assert!(!formatted.contains("Crates.io"));
}
