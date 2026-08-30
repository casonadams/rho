use crate::tools::web::search::result::SearchResult;
use scraper::{Html, Selector};

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
    let block_sel = Selector::parse("div.algo-sr, div.Sr, div.dd").unwrap();
    let link_sel = Selector::parse("a[href]").unwrap();
    let title_sel = Selector::parse("h3, a.title").unwrap();
    let snippet_sel = Selector::parse(".compText, p").unwrap();

    let mut results = Vec::new();
    for block in document.select(&block_sel) {
        let url = block
            .select(&link_sel)
            .next()
            .and_then(|a| a.value().attr("href"))
            .map(decode_yahoo_url);

        let title = block
            .select(&title_sel)
            .next()
            .map(|t| t.text().collect::<Vec<_>>().join(" "));

        let (Some(u), Some(t)) = (url, title) else {
            continue;
        };

        if u.starts_with("http") {
            let abstract_text = block
                .select(&snippet_sel)
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
