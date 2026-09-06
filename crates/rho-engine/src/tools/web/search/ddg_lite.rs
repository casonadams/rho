use std::sync::LazyLock;

use crate::tools::web::http::{HttpRequest, LYNX_UA};
use crate::tools::web::search::engine::EngineRequest;
use crate::tools::web::search::query::urlencoding_encode;
use crate::tools::web::search::result::SearchResult;
use rho_harness_core::args::WebSearchRecency;
use rho_harness_core::error::AppError;
use scraper::{Html, Selector};
use url::Url;

static LINK_SEL: LazyLock<Selector> = LazyLock::new(|| Selector::parse("a.result-link").expect("valid selector"));
static SNIPPET_SEL: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("td.result-snippet").expect("valid selector"));

pub async fn search_ddg_lite(req: &EngineRequest<'_>) -> Result<Vec<SearchResult>, AppError> {
    let df_param = match req.recency {
        Some(WebSearchRecency::Day) => "&df=d",
        Some(WebSearchRecency::Week) => "&df=w",
        Some(WebSearchRecency::Month) => "&df=m",
        Some(WebSearchRecency::Year) => "&df=y",
        None => "",
    };
    let url = format!(
        "https://lite.duckduckgo.com/lite/?q={}&kl={}{df_param}",
        urlencoding_encode(req.query),
        urlencoding_encode(req.region)
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
    Ok(parse_ddg_lite_html(&html))
}

pub fn decode_ddg_url(raw: &str) -> String {
    let Ok(u) = Url::parse(raw) else {
        return raw.to_string();
    };
    if let Some((_, target)) = u.query_pairs().find(|(k, _)| k == "uddg") {
        return target.to_string();
    }
    raw.to_string()
}

fn normalize_ddg_href(href: &str) -> String {
    if href.starts_with("//") {
        format!("https:{href}")
    } else if href.starts_with('/') {
        format!("https://lite.duckduckgo.com{href}")
    } else {
        href.to_string()
    }
}

fn build_ddg_result(link: scraper::ElementRef<'_>, snippet: Option<&String>) -> Option<SearchResult> {
    let href = link.value().attr("href")?;
    if href.is_empty() {
        return None;
    }
    let decoded = decode_ddg_url(&normalize_ddg_href(href));
    let title = link.text().collect::<Vec<_>>().join(" ").trim().to_string();
    if !decoded.is_empty() && !title.is_empty() && decoded.starts_with("http") {
        let abs = snippet.cloned().unwrap_or_default();
        Some(SearchResult::new(title, abs, decoded))
    } else {
        None
    }
}

pub fn parse_ddg_lite_html(html: &str) -> Vec<SearchResult> {
    let document = Html::parse_document(html);
    let snippets: Vec<String> = document
        .select(&SNIPPET_SEL)
        .map(|s| s.text().collect::<Vec<_>>().join(" ").trim().to_string())
        .collect();

    document
        .select(&LINK_SEL)
        .enumerate()
        .filter_map(|(i, link)| build_ddg_result(link, snippets.get(i)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ddg_lite_html() {
        let html = r#"
            <table>
                <tr>
                    <td><a class="result-link" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.rust-lang.org%2F">Rust Language</a></td>
                </tr>
                <tr>
                    <td class="result-snippet">A language empowering everyone to build reliable software.</td>
                </tr>
            </table>
        "#;
        let res = parse_ddg_lite_html(html);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].title, "Rust Language");
        assert_eq!(res[0].url, "https://www.rust-lang.org/");
        assert!(res[0].abstract_text.contains("reliable software"));
    }
}
