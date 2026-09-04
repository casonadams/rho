use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchResult {
    pub title: String,
    pub abstract_text: String,
    pub url: String,
}

impl SearchResult {
    pub fn new(title: impl Into<String>, abstract_text: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            title: title.into().trim().to_string(),
            abstract_text: abstract_text.into().trim().to_string(),
            url: url.into().trim().to_string(),
        }
    }
}

pub struct FormatResultsParams<'a> {
    pub query: &'a str,
    pub results: &'a [SearchResult],
    pub limit: usize,
    pub today: &'a str,
}

pub fn deduplicate_results(results: Vec<SearchResult>) -> Vec<SearchResult> {
    let mut seen_domains = HashSet::new();
    let mut seen_urls = HashSet::new();
    let mut deduped = Vec::new();

    for r in results {
        if seen_urls.contains(&r.url) {
            continue;
        }
        seen_urls.insert(r.url.clone());

        if let Ok(u) = url::Url::parse(&r.url)
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

pub fn format_search_results(params: FormatResultsParams<'_>) -> String {
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
