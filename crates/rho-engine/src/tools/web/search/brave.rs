use std::sync::LazyLock;

use crate::tools::web::http::{BRAVE_CHROME_UA, HttpRequest};
use crate::tools::web::search::engine::EngineRequest;
use crate::tools::web::search::query::urlencoding_encode;
use crate::tools::web::search::result::SearchResult;
use rho_harness_core::args::WebSearchRecency;
use rho_harness_core::error::AppError;
use scraper::{Html, Selector};

static SNIPPET_SEL: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse(r#"div.snippet[data-type="web"], div.snippet"#).expect("valid selector"));
static LINK_SEL: LazyLock<Selector> = LazyLock::new(|| Selector::parse("a[href]").expect("valid selector"));
static TITLE_SEL: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("div.title, a.title, h2").expect("valid selector"));
static CONTENT_SEL: LazyLock<Selector> = LazyLock::new(|| {
    Selector::parse("div.content, p.snippet-description, div.snippet-description").expect("valid selector")
});

pub async fn search_brave(req: &EngineRequest<'_>) -> Result<Vec<SearchResult>, AppError> {
    let tf_param = match req.recency {
        Some(WebSearchRecency::Day) => "&tf=pd",
        Some(WebSearchRecency::Week) => "&tf=pw",
        Some(WebSearchRecency::Month) => "&tf=pm",
        Some(WebSearchRecency::Year) => "&tf=py",
        None => "",
    };
    let url = format!(
        "https://search.brave.com/search?q={}&source=web{tf_param}",
        urlencoding_encode(req.query)
    );
    let (html, _) = req
        .http
        .get_text(HttpRequest {
            url: &url,
            user_agent: Some(BRAVE_CHROME_UA),
            timeout_sec: req.timeout_sec,
            max_bytes: 2_000_000,
        })
        .await?;
    Ok(parse_brave_html(&html))
}

pub fn parse_brave_html(html: &str) -> Vec<SearchResult> {
    let document = Html::parse_document(html);

    let mut results = Vec::new();
    for block in document.select(&SNIPPET_SEL) {
        let url = block
            .select(&LINK_SEL)
            .next()
            .and_then(|a| a.value().attr("href"))
            .map(|s| s.to_string());

        let title = block
            .select(&TITLE_SEL)
            .next()
            .map(|t| t.text().collect::<Vec<_>>().join(" "));

        let (Some(u), Some(t)) = (url, title) else {
            continue;
        };

        if u.starts_with("http") {
            let abstract_text = block
                .select(&CONTENT_SEL)
                .next()
                .map(|c| c.text().collect::<Vec<_>>().join(" "))
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
    fn test_parse_brave_html() {
        let html = r#"
            <div class="snippet" data-type="web">
                <a href="https://example.com/rust">
                    <div class="title">Rust Programming</div>
                </a>
                <div class="content">A systems language that empowers everyone.</div>
            </div>
        "#;
        let res = parse_brave_html(html);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].title, "Rust Programming");
        assert_eq!(res[0].url, "https://example.com/rust");
        assert!(res[0].abstract_text.contains("systems language"));
    }
}
