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
    for included in [
        "    2\tline 2",
        "    3\tline 3",
        "[Lines 2-3 of 5 total lines from https://example.com]",
    ] {
        assert!(res.content.contains(included));
    }
    for excluded in ["line 1", "line 4"] {
        assert!(!res.content.contains(excluded));
    }
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
