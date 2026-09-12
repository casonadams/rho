use super::result::SearchResult;
use url::Url;

pub struct FormatResultsParams<'a> {
    pub query: &'a str,
    pub results: &'a [SearchResult],
    pub limit: usize,
    pub today: &'a str,
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
