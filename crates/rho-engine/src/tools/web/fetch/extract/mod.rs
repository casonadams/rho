pub mod data;
pub mod feed;
pub mod html;

#[cfg(test)]
mod tests;

pub use data::{extract_csv, extract_json, extract_pdf_bytes};
pub use feed::extract_feed_or_xml;
pub use html::{extract_html, resolve_markdown_links};

pub fn is_pdf_request(url: &str, format_override: Option<&str>) -> bool {
    format_override == Some("pdf") || url.to_lowercase().ends_with(".pdf") || url.to_lowercase().contains(".pdf?")
}

pub struct ExtractTextParams<'a> {
    pub body: &'a str,
    pub content_type: &'a str,
    pub url_str: &'a str,
    pub mode: &'a str,
    pub format_override: Option<&'a str>,
}

pub fn extract_text(params: ExtractTextParams<'_>) -> String {
    let ct_lower = params.content_type.to_lowercase();

    if let Some(fmt) = params.format_override {
        match fmt.to_lowercase().as_str() {
            "json" => return extract_json(params.body),
            "csv" | "tsv" => return extract_csv(params.body, if fmt == "tsv" { b'\t' } else { b',' }),
            "xml" | "rss" | "atom" => return extract_feed_or_xml(params.body, params.url_str),
            "markdown" | "md" => return resolve_markdown_links(params.body, params.url_str),
            _ => {}
        }
    }

    if ct_lower.contains("json") {
        extract_json(params.body)
    } else if ct_lower.contains("xml") || ct_lower.contains("rss") || ct_lower.contains("atom") {
        extract_feed_or_xml(params.body, params.url_str)
    } else if ct_lower.contains("csv") || ct_lower.contains("tab-separated") {
        let delim = if ct_lower.contains("tab-separated") || params.url_str.ends_with(".tsv") {
            b'\t'
        } else {
            b','
        };
        extract_csv(params.body, delim)
    } else if ct_lower.contains("markdown") || params.url_str.ends_with(".md") {
        resolve_markdown_links(params.body, params.url_str)
    } else {
        extract_html(params.body, params.url_str, params.mode)
    }
}
