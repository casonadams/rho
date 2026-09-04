use super::format::{FormatFetchParams, format_fetch_output};

#[test]
fn test_format_fetch_output_empty() {
    let res = format_fetch_output(FormatFetchParams {
        text: "",
        offset: 1,
        limit: 10,
        url_str: "https://example.com",
    });
    assert_eq!(res.content, "[Empty content returned from URL]");
}

#[test]
fn test_format_fetch_output_pagination() {
    let content = "line 1\nline 2\nline 3\nline 4\nline 5";
    let res = format_fetch_output(FormatFetchParams {
        text: content,
        offset: 2,
        limit: 2,
        url_str: "https://example.com",
    });
    assert!(res.content.contains("    2\tline 2"));
    assert!(res.content.contains("    3\tline 3"));
    assert!(!res.content.contains("line 1"));
    assert!(!res.content.contains("line 4"));
    assert!(
        res.content
            .contains("[Lines 2-3 of 5 total lines from https://example.com]")
    );
}

#[test]
fn test_format_fetch_output_all_lines() {
    let content = "first\nsecond";
    let res = format_fetch_output(FormatFetchParams {
        text: content,
        offset: 1,
        limit: 10,
        url_str: "https://example.com",
    });
    assert!(res.content.contains("    1\tfirst"));
    assert!(res.content.contains("    2\tsecond"));
    assert!(!res.content.contains("[Lines"));
}
