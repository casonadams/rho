use std::sync::LazyLock;

use crate::tools::web::http::BRAVE_CHROME_UA;
use crate::tools::web::search::engine::{EngineRequest, fetch_results_page};
use crate::tools::web::search::query::{recency_param, urlencoding_encode};
use crate::tools::web::search::result::{ResultSelectors, SearchResult, parse_result_blocks};
use crate::tools::web::selector;
use rho_harness_core::args::WebSearchRecency;
use rho_harness_core::error::AppError;

static SELECTORS: LazyLock<ResultSelectors> = LazyLock::new(|| ResultSelectors {
    block: selector(r#"div.snippet[data-type="web"], div.snippet"#),
    title: selector("div.title, a.title, h2"),
    snippet: selector("div.content, p.snippet-description, div.snippet-description"),
});

pub(crate) fn brave_search_url(query: &str, recency: Option<WebSearchRecency>) -> String {
    let tf_param = recency_param(recency, "tf", ["pd", "pw", "pm", "py"]);
    format!(
        "https://search.brave.com/search?q={}&source=web{tf_param}",
        urlencoding_encode(query)
    )
}

pub(crate) async fn search_brave(req: &EngineRequest<'_>) -> Result<Vec<SearchResult>, AppError> {
    let url = brave_search_url(req.query, req.recency);
    let body = fetch_results_page(req, &url, BRAVE_CHROME_UA).await?;
    Ok(parse_brave_html(&body))
}

pub(crate) fn parse_brave_html(html: &str) -> Vec<SearchResult> {
    parse_result_blocks(html, &SELECTORS, |href| href.to_string())
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

    #[test]
    fn test_brave_search_url_recency() {
        assert_eq!(
            brave_search_url("rust lang", None),
            "https://search.brave.com/search?q=rust%20lang&source=web"
        );
        assert_eq!(
            brave_search_url("rust lang", Some(WebSearchRecency::Day)),
            "https://search.brave.com/search?q=rust%20lang&source=web&tf=pd"
        );
        assert_eq!(
            brave_search_url("rust lang", Some(WebSearchRecency::Week)),
            "https://search.brave.com/search?q=rust%20lang&source=web&tf=pw"
        );
        assert_eq!(
            brave_search_url("rust lang", Some(WebSearchRecency::Month)),
            "https://search.brave.com/search?q=rust%20lang&source=web&tf=pm"
        );
        assert_eq!(
            brave_search_url("rust lang", Some(WebSearchRecency::Year)),
            "https://search.brave.com/search?q=rust%20lang&source=web&tf=py"
        );
    }
}
