use super::data::{extract_csv, extract_json};
use super::feed::extract_feed_or_xml;
use super::html::{extract_html, resolve_markdown_links};
use super::*;

#[test]
fn test_extract_html() {
    let html = "<html><body><h1>Hello World</h1><nav>Skip me</nav><p>Content paragraph</p></body></html>";
    let res = extract_html(html, "https://example.com", "main");
    assert!(res.contains("Hello World"));
    assert!(res.contains("Content paragraph"));
    assert!(!res.contains("Skip me"));
}

#[test]
fn test_extract_json() {
    let json = r#"{"name":"test","count":42}"#;
    let res = extract_json(json);
    assert!(res.contains("\"name\": \"test\""));
    assert!(res.contains("\"count\": 42"));
}

#[test]
fn test_extract_csv() {
    let csv_data = "name,age,city\nAlice,30,NYC\nBob,25,SF\n";
    let res = extract_csv(csv_data, b',');
    assert!(res.contains("| name | age | city |"));
    assert!(res.contains("| Alice | 30 | NYC |"));
}

#[test]
fn test_extract_tsv() {
    let tsv_data = "name\tage\tcity\nAlice\t30\tNYC\n";
    let res = extract_csv(tsv_data, b'\t');
    assert!(res.contains("| name | age | city |"));
    assert!(res.contains("| Alice | 30 | NYC |"));
}

#[test]
fn test_resolve_markdown_links() {
    let md = "[Doc](/docs/guide.md) and [External](https://example.com/api)";
    let res = resolve_markdown_links(md, "https://example.com/sub/");
    assert!(res.contains("[Doc](https://example.com/docs/guide.md)"));
    assert!(res.contains("[External](https://example.com/api)"));
}

#[test]
fn test_is_pdf_request() {
    assert!(is_pdf_request("https://example.com/doc.pdf", None));
    assert!(is_pdf_request("https://example.com/doc.pdf?dl=1", None));
    assert!(is_pdf_request("https://example.com/fetch", Some("pdf")));
    assert!(!is_pdf_request("https://example.com/page", None));
    assert!(!is_pdf_request("https://example.com/page", Some("html")));
}

#[test]
fn test_extract_feed() {
    let rss = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Sample Feed</title>
    <description>A test feed</description>
    <item>
      <title>First Post</title>
      <link>https://example.com/first</link>
      <description>Summary of first post</description>
    </item>
  </channel>
</rss>"#;
    let res = extract_feed_or_xml(rss, "https://example.com");
    assert!(res.contains("# Sample Feed"));
    assert!(res.contains("## First Post"));
    assert!(res.contains("Link: https://example.com/first"));
    assert!(res.contains("Summary of first post"));
}

#[test]
fn test_extract_sitemap() {
    let sitemap = r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url><loc>https://example.com/page1</loc></url>
  <url><loc>https://example.com/page2</loc></url>
</urlset>"#;
    let res = extract_feed_or_xml(sitemap, "https://example.com");
    assert!(res.contains("Sitemap containing 2 URLs:"));
    assert!(res.contains("- https://example.com/page1"));
    assert!(res.contains("- https://example.com/page2"));
}

#[test]
fn test_extract_text_routing() {
    let json_body = r#"{"hello":"world"}"#;
    let out = extract_text(ExtractTextParams {
        body: json_body,
        content_type: "application/json",
        url_str: "https://example.com/data",
        mode: "auto",
        format_override: None,
    });
    assert!(out.contains("\"hello\": \"world\""));

    let out_override = extract_text(ExtractTextParams {
        body: json_body,
        content_type: "text/plain",
        url_str: "https://example.com/data",
        mode: "auto",
        format_override: Some("json"),
    });
    assert!(out_override.contains("\"hello\": \"world\""));

    let csv_body = "col1,col2\nval1,val2";
    let out_csv = extract_text(ExtractTextParams {
        body: csv_body,
        content_type: "text/csv",
        url_str: "https://example.com/data.csv",
        mode: "auto",
        format_override: None,
    });
    assert!(out_csv.contains("| col1 | col2 |"));
}
