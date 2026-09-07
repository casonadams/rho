use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchResult {
    pub title: String,
    pub abstract_text: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl SearchResult {
    pub fn new(title: impl Into<String>, abstract_text: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            title: title.into().trim().to_string(),
            abstract_text: abstract_text.into().trim().to_string(),
            url: url.into().trim().to_string(),
            content_hint: None,
            source: None,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.content_hint = Some(hint.into());
        self
    }

    pub fn with_source(mut self, src: impl Into<String>) -> Self {
        self.source = Some(src.into());
        self
    }
}

pub struct FormatResultsParams<'a> {
    pub query: &'a str,
    pub results: &'a [SearchResult],
    pub limit: usize,
    pub today: &'a str,
}

fn is_tracking_param(key: &str) -> bool {
    let lower = key.to_lowercase();
    lower.starts_with("utm_")
        || matches!(
            lower.as_str(),
            "dclid" | "fbclid" | "gclid" | "mc_cid" | "mc_eid" | "msclkid"
        )
}

fn detect_content_hint(url: &Url) -> Option<&'static str> {
    let path = url.path().to_lowercase();
    if path.ends_with(".pdf") {
        return Some("PDF");
    }
    if path.ends_with(".json") {
        return Some("JSON");
    }
    let host = url.host_str().unwrap_or("");
    if host == "github.com" || host.ends_with(".github.com") || host == "raw.githubusercontent.com" {
        return Some("GitHub");
    }
    if host.starts_with("docs.")
        || host == "developer.mozilla.org"
        || path.contains("/docs/")
        || path.contains("/documentation/")
        || path.contains("/reference/")
        || path.contains("/api/")
    {
        return Some("documentation");
    }
    None
}

fn rewrite_github_blob(url: &mut Url) {
    if url.host_str() != Some("github.com") {
        return;
    }
    let parts: Vec<String> = url
        .path()
        .split('/')
        .filter(|p| !p.is_empty())
        .map(ToString::to_string)
        .collect();
    if parts.len() >= 5 && parts[2] == "blob" {
        let user = &parts[0];
        let repo = &parts[1];
        let rest = parts[3..].join("/");
        let _ = url.set_host(Some("raw.githubusercontent.com"));
        url.set_path(&format!("/{user}/{repo}/{rest}"));
        url.set_query(None);
    }
}

fn clean_query_params(url: &mut Url) {
    let mut pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| !is_tracking_param(k))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    pairs.sort();
    url.query_pairs_mut().clear();
    for (k, v) in &pairs {
        url.query_pairs_mut().append_pair(k, v);
    }
    if pairs.is_empty() {
        url.set_query(None);
    }
}

fn canonicalize_url(raw_url: &str) -> Option<(Url, Option<&'static str>)> {
    let mut url = Url::parse(raw_url).ok()?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return None;
    }
    url.set_fragment(None);
    rewrite_github_blob(&mut url);
    clean_query_params(&mut url);
    let hint = detect_content_hint(&url);
    Some((url, hint))
}

fn dedupe_key(url: &Url) -> String {
    let host = url.host_str().unwrap_or("").to_lowercase();
    let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
    let path = if url.path().len() > 1 {
        url.path().trim_end_matches('/')
    } else {
        url.path()
    };
    let query = url.query().map(|q| format!("?{q}")).unwrap_or_default();
    format!("{host}{port}{path}{query}")
}

pub fn deduplicate_results(results: Vec<SearchResult>) -> Vec<SearchResult> {
    let mut by_key: HashMap<String, SearchResult> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for mut r in results {
        let Some((url, hint)) = canonicalize_url(&r.url) else {
            continue;
        };
        let key = dedupe_key(&url);
        r.url = url.to_string();
        if r.content_hint.is_none() {
            r.content_hint = hint.map(ToString::to_string);
        }

        if let Some(existing) = by_key.get_mut(&key) {
            if existing.url.starts_with("http:") && r.url.starts_with("https:") {
                *existing = r;
            }
        } else {
            order.push(key.clone());
            by_key.insert(key, r);
        }
    }

    order.into_iter().filter_map(|k| by_key.remove(&k)).collect()
}

fn format_signals(r: &SearchResult) -> String {
    let mut signals: Vec<String> = Vec::new();
    if let Ok(u) = Url::parse(&r.url)
        && let Some(host) = u.host_str()
    {
        signals.push(host.strip_prefix("www.").unwrap_or(host).to_string());
    }
    if let Some(h) = r.content_hint.as_deref() {
        signals.push(h.to_string());
    }
    if let Some(s) = r.source.as_deref() {
        signals.push(s.to_string());
    }
    if !signals.is_empty() {
        format!(" ({})", signals.join(" | "))
    } else {
        String::new()
    }
}

fn format_via_engines(results: &[SearchResult], limit: usize) -> String {
    let mut engines: Vec<&str> = Vec::new();
    for r in results.iter().take(limit) {
        if let Some(src) = r.source.as_deref()
            && !engines.contains(&src)
        {
            engines.push(src);
        }
    }
    if !engines.is_empty() {
        format!(" via {}", engines.join(" + "))
    } else {
        String::new()
    }
}

fn format_search_item(out: &mut String, idx: usize, r: &SearchResult) {
    let metadata = format_signals(r);
    out.push_str(&format!("{idx}. **{}**{metadata}\n   URL: {}\n", r.title, r.url));
    if !r.abstract_text.is_empty() {
        out.push_str(&format!("   Summary: {}\n", r.abstract_text));
    }
    out.push('\n');
}

pub fn format_search_results(params: FormatResultsParams<'_>) -> String {
    let via = format_via_engines(params.results, params.limit);
    let mut out = format!(
        "**Search results for:** {}{via} (searched on {})\n\n",
        params.query, params.today
    );
    for (i, r) in params.results.iter().take(params.limit).enumerate() {
        format_search_item(&mut out, i + 1, r);
    }
    out.trim_end().to_string()
}
