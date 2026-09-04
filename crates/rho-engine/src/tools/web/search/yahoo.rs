use std::sync::LazyLock;

use crate::tools::web::http::{HttpRequest, LYNX_UA};
use crate::tools::web::search::engine::EngineRequest;
use crate::tools::web::search::query::urlencoding_encode;
use crate::tools::web::search::result::SearchResult;
use rho_harness_core::args::WebSearchRecency;
use rho_harness_core::error::AppError;
use scraper::{Html, Selector};

static BLOCK_SEL: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("div.algo-sr, div.Sr, div.dd").expect("valid selector"));
static LINK_SEL: LazyLock<Selector> = LazyLock::new(|| Selector::parse("a[href]").expect("valid selector"));
static TITLE_SEL: LazyLock<Selector> = LazyLock::new(|| Selector::parse("h3, a.title").expect("valid selector"));
static SNIPPET_SEL: LazyLock<Selector> = LazyLock::new(|| Selector::parse(".compText, p").expect("valid selector"));

pub async fn search_yahoo(req: &EngineRequest<'_>) -> Result<Vec<SearchResult>, AppError> {
    let age_param = match req.recency {
        Some(WebSearchRecency::Day) => "&age=1d",
        Some(WebSearchRecency::Week) => "&age=1w",
        Some(WebSearchRecency::Month) => "&age=1m",
        Some(WebSearchRecency::Year) => "&age=1y",
        None => "",
    };
    let url = format!(
        "https://search.yahoo.com/search?p={}{age_param}",
        urlencoding_encode(req.query)
    );
    let (html, _) = req
        .http
        .get_text(HttpRequest {
            url: &url,
            user_agent: Some(LYNX_UA),
            timeout_sec: req.timeout_sec,
            max_bytes: 2_000_000,
        })
        .await?;
    Ok(parse_yahoo_html(&html))
}

pub fn decode_yahoo_url(raw: &str) -> String {
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

pub fn parse_yahoo_html(html: &str) -> Vec<SearchResult> {
    let document = Html::parse_document(html);

    let mut results = Vec::new();
    for block in document.select(&BLOCK_SEL) {
        let url = block
            .select(&LINK_SEL)
            .next()
            .and_then(|a| a.value().attr("href"))
            .map(decode_yahoo_url);

        let title = block
            .select(&TITLE_SEL)
            .next()
            .map(|t| t.text().collect::<Vec<_>>().join(" "));

        let (Some(u), Some(t)) = (url, title) else {
            continue;
        };

        if u.starts_with("http") {
            let abstract_text = block
                .select(&SNIPPET_SEL)
                .next()
                .map(|s| s.text().collect::<Vec<_>>().join(" "))
                .unwrap_or_default();

            results.push(SearchResult::new(t, abstract_text, u));
        }
    }
    results
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
}
