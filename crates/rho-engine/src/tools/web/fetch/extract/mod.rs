pub mod data;
pub mod feed;
pub mod html;
pub mod markdown;

#[cfg(test)]
mod tests;

pub use data::{extract_csv, extract_json, extract_pdf_bytes};
pub use feed::extract_feed_or_xml;
pub use html::extract_html;
pub use markdown::resolve_markdown_links;
use rho_harness_core::error::{AppError, Result};

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

fn extract_delimited(body: &str, ct_lower: &str, url_str: &str) -> String {
    let delim = if ct_lower.contains("tab-separated") || url_str.ends_with(".tsv") {
        b'\t'
    } else {
        b','
    };
    extract_csv(body, delim)
}

fn is_markdown(ct_lower: &str, url_str: &str) -> bool {
    ct_lower.contains("markdown") || url_str.ends_with(".md") || url_str.ends_with(".markdown")
}

fn is_csv(ct_lower: &str, url_str: &str) -> bool {
    ct_lower.contains("csv")
        || ct_lower.contains("tab-separated")
        || url_str.ends_with(".csv")
        || url_str.ends_with(".tsv")
}

fn is_html(ct_lower: &str, body: &str) -> bool {
    ct_lower.contains("html")
        || ct_lower.contains("xhtml")
        || body.trim_start().to_ascii_lowercase().starts_with("<!doctype html")
        || body.trim_start().to_ascii_lowercase().starts_with("<html")
}

fn extract_override_text(fmt: &str, body: &str, url_str: &str, mode: &str) -> Option<Result<String>> {
    match fmt.to_lowercase().as_str() {
        "json" => Some(Ok(extract_json(body))),
        "csv" | "tsv" => Some(Ok(extract_csv(body, if fmt == "tsv" { b'\t' } else { b',' }))),
        "xml" | "rss" | "atom" => Some(Ok(extract_feed_or_xml(body, url_str))),
        "markdown" | "md" => Some(Ok(resolve_markdown_links(body, url_str))),
        "html" => Some(extract_html(body, url_str, mode)),
        _ => None,
    }
}

fn extract_structured_text(body: &str, ct_lower: &str, url_str: &str) -> Option<String> {
    if is_markdown(ct_lower, url_str) {
        Some(resolve_markdown_links(body.trim(), url_str))
    } else if ct_lower.contains("json") {
        Some(extract_json(body))
    } else if ct_lower.contains("xml") || body.trim_start().starts_with("<?xml") {
        Some(extract_feed_or_xml(body, url_str))
    } else if is_csv(ct_lower, url_str) {
        Some(extract_delimited(body, ct_lower, url_str))
    } else {
        None
    }
}

fn extract_inferred_text(body: &str, ct_lower: &str, url_str: &str, mode: &str) -> Result<String> {
    if let Some(text) = extract_structured_text(body, ct_lower, url_str) {
        return Ok(text);
    }
    if is_html(ct_lower, body) {
        return extract_html(body, url_str, mode);
    }
    if !ct_lower.is_empty() && !ct_lower.starts_with("text/") {
        let prefix = ct_lower.split(';').next().unwrap_or(ct_lower);
        return Err(AppError::Tool(format!("Unsupported content type: {prefix}")));
    }
    Ok(body.trim().to_string())
}

pub fn extract_text(params: ExtractTextParams<'_>) -> Result<String> {
    let ct_lower = params.content_type.to_lowercase();
    if let Some(fmt) = params.format_override
        && let Some(text_res) = extract_override_text(fmt, params.body, params.url_str, params.mode)
    {
        return text_res;
    }
    extract_inferred_text(params.body, &ct_lower, params.url_str, params.mode)
}
