use std::sync::LazyLock;

use crate::tools::web::http::{HttpRequest, LYNX_UA};
use crate::tools::web::search::engine::EngineRequest;
use crate::tools::web::search::query::urlencoding_encode;
use crate::tools::web::search::result::SearchResult;
use rho_harness_core::args::WebSearchRecency;
use rho_harness_core::error::AppError;
use scraper::{Html, Selector};
use url::Url;

static TR_SEL: LazyLock<Selector> = LazyLock::new(|| Selector::parse("tr").expect("valid selector"));
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
    let resp = req
        .http
        .get_text(HttpRequest {
            url: &url,
            user_agent: Some(LYNX_UA),
            timeout_sec: req.timeout_sec,
            max_bytes: 2_000_000,
            pdf_max_bytes: None,
        })
        .await?;
    Ok(parse_ddg_lite_html(&resp.body))
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

fn parse_ddg_row(rows: &[scraper::ElementRef<'_>], idx: usize) -> Option<SearchResult> {
    let row = rows.get(idx)?;
    let link = row.select(&LINK_SEL).next()?;
    let href = link.value().attr("href")?;
    if href.is_empty() {
        return None;
    }
    let decoded = decode_ddg_url(&normalize_ddg_href(href));
    let title = link.text().collect::<Vec<_>>().join(" ").trim().to_string();
    if decoded.is_empty() || title.is_empty() || !decoded.starts_with("http") {
        return None;
    }

    let snippet = rows
        .get(idx + 1)
        .and_then(|next_row| next_row.select(&SNIPPET_SEL).next())
        .map(|s| s.text().collect::<Vec<_>>().join(" ").trim().to_string())
        .unwrap_or_default();

    Some(SearchResult::new(title, snippet, decoded))
}

pub fn parse_ddg_lite_html(html: &str) -> Vec<SearchResult> {
    let document = Html::parse_document(html);
    let rows: Vec<_> = document.select(&TR_SEL).collect();
    (0..rows.len()).filter_map(|idx| parse_ddg_row(&rows, idx)).collect()
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
