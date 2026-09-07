use super::data::{extract_csv, extract_json};
use super::feed::extract_feed_or_xml;
use super::html::extract_html;
use super::markdown::resolve_markdown_links;
use super::*;

#[test]
fn test_extract_html_semantic_main() {
    let article_text = "Important article content with enough detail for extraction. ".repeat(12);
    let html = format!(
        "<html><head><title>Article Title</title><meta name=\"author\" content=\"Jane Doe\"><meta property=\"og:site_name\" content=\"TechBlog\"></head><body><nav>NOISY NAVIGATION</nav><main><h1>Article Heading</h1><p>{article_text}</p></main><footer>NOISY FOOTER</footer></body></html>"
    );
    let res = extract_html(&html, "https://example.com/post", "auto").unwrap();
    for expected in [
        "main content via <main>",
        "# Article Title",
        "By: Jane Doe",
        "Site: TechBlog",
    ] {
        assert!(res.contains(expected));
    }
    assert!(!res.contains("NOISY NAVIGATION") && !res.contains("NOISY FOOTER"));
}

#[test]
fn test_extract_html_base_href() {
    let html = "<html><head><base href=\"/assets/\"></head><body><p><a href=\"guide.html\">Guide</a></p></body></html>";
    let res = extract_html(html, "https://example.com/pages/index.html", "full").unwrap();
    assert!(res.contains("https://example.com/assets/guide.html"));
}

#[test]
fn test_extract_html_spa_shell() {
    let html = format!(
        "<html><body><div id=\"root\"></div><script>{}</script></body></html>",
        "x".repeat(600)
    );
    let res = extract_html(&html, "https://example.com/app", "auto");
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("JavaScript-capable browser"));
}

#[test]
fn test_extract_html_main_mode_missing_content() {
    let html = "<html><body><p>Short</p></body></html>";
    let res = extract_html(html, "https://example.com/short", "main");
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("mode=\"full\""));
}

#[test]
fn test_markdown_preserves_code_fences() {
    let md = [
        "[Outer](./outer.md)",
        "```md",
        "[Inner](./leave-relative.md)",
        "```",
        "[After](./after.md)",
    ]
    .join("\n");
    let res = resolve_markdown_links(&md, "https://example.com/docs/");
    assert!(res.contains("[Outer](https://example.com/docs/outer.md)"));
    assert!(res.contains("[Inner](./leave-relative.md)"));
    assert!(res.contains("[After](https://example.com/docs/after.md)"));
}

#[test]
fn test_markdown_resolves_reference_and_protocol_relative() {
    let md = ["[Asset](//cdn.example.com/asset.txt)", "[Reference]: ../ref.md"].join("\n");
    let res = resolve_markdown_links(&md, "https://example.com/docs/sub/");
    assert!(res.contains("[Asset](https://cdn.example.com/asset.txt)"));
    assert!(res.contains("[Reference]: https://example.com/docs/ref.md"));
}

#[test]
fn test_extract_csv_escapes_pipes_and_sanitizes_cells() {
    let csv = "name,desc\nitem1,contains | pipe\nitem2,\"line1\nline2\"\n";
    let res = extract_csv(csv, b',');
    assert!(res.contains(r"| item1 | contains \| pipe |"));
    assert!(res.contains("| item2 | line1 line2 |"));
}

#[test]
fn test_extract_csv_truncation_notice() {
    let mut rows = vec!["id,val".to_string()];
    for i in 0..100 {
        rows.push(format!("row{i},{i}"));
    }
    let res = extract_csv(&rows.join("\n"), b',');
    assert!(res.contains("[Truncated: showing first 49 of 100 data rows]"));
}

#[test]
fn test_extract_json() {
    let json = r#"{"name":"test","count":42}"#;
    let res = extract_json(json);
    assert!(res.contains("\"name\": \"test\""));
    assert!(res.contains("\"count\": 42"));
}

#[test]
fn test_extract_feed_atom_alternate() {
    let atom = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Atom Feed</title>
  <entry>
    <title>Post One</title>
    <link rel="self" href="/api/entry/1"/>
    <link rel="alternate" href="/posts/1"/>
    <summary>Post summary.</summary>
  </entry>
</feed>"#;
    let res = extract_feed_or_xml(atom, "https://example.com");
    assert!(res.contains("https://example.com/posts/1"));
    assert!(!res.contains("/api/entry/1"));
}

#[test]
fn test_extract_sitemap_deduplicates() {
    let sitemap = r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url><loc>https://example.com/page1</loc></url>
  <url><loc>https://example.com/page2</loc></url>
  <url><loc>https://example.com/page1</loc></url>
</urlset>"#;
    let res = extract_feed_or_xml(sitemap, "https://example.com");
    assert!(res.contains("# Sitemap\n\n1. https://example.com/page1\n2. https://example.com/page2"));
}

#[test]
fn test_extract_text_routing() {
    let json_body = r#"{"key":"val"}"#;
    let res = extract_text(ExtractTextParams {
        body: json_body,
        content_type: "application/json",
        url_str: "https://example.com/api",
        mode: "auto",
        format_override: None,
    })
    .unwrap();
    assert!(res.contains("\"key\": \"val\""));

    let unsupported = extract_text(ExtractTextParams {
        body: "binary",
        content_type: "image/png",
        url_str: "https://example.com/img.png",
        mode: "auto",
        format_override: None,
    });
    assert!(unsupported.is_err());
}
