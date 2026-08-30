use crate::tools::web::search::result::SearchResult;
use scraper::{Html, Selector};

pub fn parse_brave_html(html: &str) -> Vec<SearchResult> {
    let document = Html::parse_document(html);
    let snippet_sel = Selector::parse(r#"div.snippet[data-type="web"], div.snippet"#).unwrap();
    let link_sel = Selector::parse("a[href]").unwrap();
    let title_sel = Selector::parse("div.title, a.title, h2").unwrap();
    let content_sel = Selector::parse("div.content, p.snippet-description, div.snippet-description").unwrap();

    let mut results = Vec::new();
    for block in document.select(&snippet_sel) {
        let url = block
            .select(&link_sel)
            .next()
            .and_then(|a| a.value().attr("href"))
            .map(|s| s.to_string());

        let title = block
            .select(&title_sel)
            .next()
            .map(|t| t.text().collect::<Vec<_>>().join(" "));

        let (Some(u), Some(t)) = (url, title) else {
            continue;
        };

        if u.starts_with("http") {
            let abstract_text = block
                .select(&content_sel)
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
