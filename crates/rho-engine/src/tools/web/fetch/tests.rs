use super::encoding::*;
use super::pagination::*;
use super::*;

#[test]
fn test_format_page_empty() {
    let res = format_page(FormatPageParams {
        text: "",
        offset: 1,
        limit: 10,
        source_url: "https://example.com",
        final_url: "https://example.com",
    });
    assert_eq!(res.content, "[Empty content returned from URL]");
}

#[test]
fn test_format_page_pagination_clean_lines() {
    let content = "line 1\nline 2\nline 3\nline 4\nline 5";
    let res = format_page(FormatPageParams {
        text: content,
        offset: 2,
        limit: 2,
        source_url: "https://example.com",
        final_url: "https://example.com",
    });
    for expected in [
        "line 2\nline 3",
        "[Truncated: 5 lines total. Use offset=4 to continue.]",
    ] {
        assert!(res.content.contains(expected));
    }
    for excluded in ["line 1", "line 4", "    2\t"] {
        assert!(!res.content.contains(excluded));
    }
}

#[test]
fn test_format_page_reports_final_redirect_url() {
    let content = "final content";
    let res = format_page(FormatPageParams {
        text: content,
        offset: 1,
        limit: 10,
        source_url: "https://example.com/start",
        final_url: "https://example.com/final",
    });
    assert!(
        res.content
            .starts_with("[Final URL after redirects: https://example.com/final]\n\n")
    );
    assert!(res.content.contains("final content"));
}

#[test]
fn test_decode_utf16le_bom() {
    let bytes = [0xff, 0xfe, 0x68, 0x00, 0x69, 0x00];
    let text = decode_body(&bytes, "text/plain");
    assert_eq!(text, "hi");
}

#[test]
fn test_decode_utf8_bom() {
    let bytes = [0xef, 0xbb, 0xbf, b'h', b'e', b'l', b'l', b'o'];
    let text = decode_body(&bytes, "text/plain");
    assert_eq!(text, "hello");
}

#[test]
fn test_decode_html_meta_charset() {
    let html = b"<html><head><meta charset=\"windows-1252\"></head><body>caf\xe9</body></html>";
    let text = decode_body(html, "text/html");
    assert!(text.contains("café"));
}

#[test]
fn test_is_pdf_detection() {
    assert!(is_pdf(b"%PDF-1.4\n...", "application/octet-stream"));
    assert!(is_pdf(b"anything", "application/pdf"));
    assert!(!is_pdf(b"<html>...", "text/html"));
}

#[test]
fn test_pagination_byte_limit() {
    let large_line = "x".repeat(50_000);
    let prepared = prepare_lines(&large_line, LINE_MAX_BYTES);
    assert!(prepared.len() > 10);
    let page = page_lines(PageLinesParams {
        lines: &prepared,
        start: 0,
        line_limit: 100,
        byte_limit: PAGE_BYTE_LIMIT,
    });
    assert!(page.content.len() <= PAGE_BYTE_LIMIT);
    assert!(page.consumed < prepared.len());
}

#[tokio::test]
async fn test_web_fetch_rejects_credentials_with_explanation() {
    let http = HttpClient::new(true).unwrap();
    let cache = FetchCache::new(60, 4);
    let tool = WebFetchTool::new(
        http,
        cache,
        WebFetchConfig {
            timeout_sec: 1,
            max_bytes: 1024,
            pdf_max_bytes: 1024,
            default_limit: 20,
        },
    );
    let res = tool
        .execute(WebFetchArgs {
            url: "http://user:pass@example.com".to_string(),
            offset: None,
            limit: None,
            mode: None,
            format: None,
        })
        .await
        .unwrap();
    assert!(res.is_error);
    assert!(res.content.contains("credentials"));
}
