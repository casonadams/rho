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
