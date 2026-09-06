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

fn extract_override_text(fmt: &str, body: &str, url_str: &str) -> Option<String> {
    match fmt.to_lowercase().as_str() {
        "json" => Some(extract_json(body)),
        "csv" | "tsv" => Some(extract_csv(body, if fmt == "tsv" { b'\t' } else { b',' })),
        "xml" | "rss" | "atom" => Some(extract_feed_or_xml(body, url_str)),
        "markdown" | "md" => Some(resolve_markdown_links(body, url_str)),
        _ => None,
    }
}

fn extract_delimited(body: &str, ct_lower: &str, url_str: &str) -> String {
    let delim = if ct_lower.contains("tab-separated") || url_str.ends_with(".tsv") {
        b'\t'
    } else {
        b','
    };
    extract_csv(body, delim)
}

fn extract_inferred_text((body, ct_lower): (&str, &str), (url_str, mode): (&str, &str)) -> String {
    if ct_lower.contains("json") {
        extract_json(body)
    } else if ct_lower.contains("xml") || ct_lower.contains("rss") || ct_lower.contains("atom") {
        extract_feed_or_xml(body, url_str)
    } else if ct_lower.contains("csv") || ct_lower.contains("tab-separated") {
        extract_delimited(body, ct_lower, url_str)
    } else if ct_lower.contains("markdown") || url_str.ends_with(".md") {
        resolve_markdown_links(body, url_str)
    } else {
        extract_html(body, url_str, mode)
    }
}

pub fn extract_text(params: ExtractTextParams<'_>) -> String {
    let ct_lower = params.content_type.to_lowercase();
    if let Some(fmt) = params.format_override
        && let Some(text) = extract_override_text(fmt, params.body, params.url_str)
    {
        return text;
    }
    extract_inferred_text((params.body, &ct_lower), (params.url_str, params.mode))
}
