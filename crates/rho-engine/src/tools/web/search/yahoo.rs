use std::sync::LazyLock;

use crate::tools::web::http::LYNX_UA;
use crate::tools::web::search::engine::{EngineRequest, fetch_results_page};
use crate::tools::web::search::query::{recency_param, urlencoding_encode};
use crate::tools::web::search::result::{ResultSelectors, SearchResult, parse_result_blocks};
use crate::tools::web::selector;
use rho_harness_core::args::WebSearchRecency;
use rho_harness_core::error::AppError;

static SELECTORS: LazyLock<ResultSelectors> = LazyLock::new(|| ResultSelectors {
    block: selector("div.algo-sr, div.Sr, div.dd"),
    title: selector("h3, a.title"),
    snippet: selector(".compText, p"),
});

pub(crate) fn yahoo_search_url(query: &str, recency: Option<WebSearchRecency>) -> String {
    let age_param = recency_param(recency, "age", ["1d", "1w", "1m", "1y"]);
    format!(
        "https://search.yahoo.com/search?p={}{age_param}",
        urlencoding_encode(query)
    )
}

pub(crate) async fn search_yahoo(req: &EngineRequest<'_>) -> Result<Vec<SearchResult>, AppError> {
    let url = yahoo_search_url(req.query, req.recency);
    let body = fetch_results_page(req, &url, LYNX_UA).await?;
    Ok(parse_yahoo_html(&body))
}

pub(crate) fn decode_yahoo_url(raw: &str) -> String {
    if let Some(pos) = raw.find("/RU=") {
        let remainder = &raw[pos + 4..];
        let end = remainder.find('/').unwrap_or(remainder.len());
        let encoded = &remainder[..end];
        if let Ok(decoded) = urlencoding_decode(encoded)
            && decoded.starts_with("http")
        {
            return decoded;
        }
    }
    raw.to_string()
}

fn urlencoding_decode(s: &str) -> Result<String, ()> {
    percent_encoding::percent_decode_str(s)
        .decode_utf8()
        .map(|cow| cow.into_owned())
        .map_err(|_| ())
}

pub(crate) fn parse_yahoo_html(html: &str) -> Vec<SearchResult> {
    parse_result_blocks(html, &SELECTORS, decode_yahoo_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_yahoo_html() {
        let html = r#"
            <div class="algo-sr">
                <h3><a href="https://r.search.yahoo.com/_ylt=.../RU=https%3a%2f%2fdocs.rs%2f/RK=2/...">Docs.rs</a></h3>
                <div class="compText">Documentation for crates in Rust.</div>
            </div>
        "#;
        let res = parse_yahoo_html(html);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].title, "Docs.rs");
        assert_eq!(res[0].url, "https://docs.rs/");
        assert!(res[0].abstract_text.contains("Documentation"));
    }

    #[test]
    fn test_yahoo_search_url_recency() {
        assert_eq!(
            yahoo_search_url("rust lang", None),
            "https://search.yahoo.com/search?p=rust%20lang"
        );
        assert_eq!(
            yahoo_search_url("rust lang", Some(WebSearchRecency::Day)),
            "https://search.yahoo.com/search?p=rust%20lang&age=1d"
        );
        assert_eq!(
            yahoo_search_url("rust lang", Some(WebSearchRecency::Week)),
            "https://search.yahoo.com/search?p=rust%20lang&age=1w"
        );
        assert_eq!(
            yahoo_search_url("rust lang", Some(WebSearchRecency::Month)),
            "https://search.yahoo.com/search?p=rust%20lang&age=1m"
        );
        assert_eq!(
            yahoo_search_url("rust lang", Some(WebSearchRecency::Year)),
            "https://search.yahoo.com/search?p=rust%20lang&age=1y"
        );
    }
}
