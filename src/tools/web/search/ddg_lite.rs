use crate::tools::web::search::result::SearchResult;
use scraper::{Html, Selector};
use url::Url;

pub fn decode_ddg_url(raw: &str) -> String {
    let Ok(u) = Url::parse(raw) else {
        return raw.to_string();
    };
    if let Some((_, target)) = u.query_pairs().find(|(k, _)| k == "uddg") {
        return target.to_string();
    }
    raw.to_string()
}

pub fn parse_ddg_lite_html(html: &str) -> Vec<SearchResult> {
    let document = Html::parse_document(html);
    let link_sel = Selector::parse("a.result-link").unwrap();
    let snippet_sel = Selector::parse("td.result-snippet").unwrap();

    let mut results = Vec::new();
    let snippets: Vec<String> = document
        .select(&snippet_sel)
        .map(|s| s.text().collect::<Vec<_>>().join(" ").trim().to_string())
        .collect();

    for (i, link) in document.select(&link_sel).enumerate() {
        let href = link.value().attr("href").unwrap_or_default();
        if href.is_empty() {
            continue;
        }

        let full_url = if href.starts_with("//") {
            format!("https:{href}")
        } else if href.starts_with('/') {
            format!("https://lite.duckduckgo.com{href}")
        } else {
            href.to_string()
        };

        let decoded_url = decode_ddg_url(&full_url);
        let title = link.text().collect::<Vec<_>>().join(" ").trim().to_string();
        let abstract_text = snippets.get(i).cloned().unwrap_or_default();

        if !decoded_url.is_empty() && !title.is_empty() && decoded_url.starts_with("http") {
            results.push(SearchResult::new(title, abstract_text, decoded_url));
        }
    }
    results
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
