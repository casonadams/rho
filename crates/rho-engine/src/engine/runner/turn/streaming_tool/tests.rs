use super::*;

#[test]
fn extract_path_from_partial_json() {
    let json = r#"{"path": "src/main.rs", "content": "hello"#;
    assert_eq!(extract_json_string_field(json, "path").as_deref(), Some("src/main.rs"));

    let incomplete = r#"{"path": "src/main"#;
    assert_eq!(extract_json_string_field(incomplete, "path"), None);

    let escaped = r#"{"path": "foo/bar\"baz/test.rs"}"#;
    assert_eq!(
        extract_json_string_field(escaped, "path").as_deref(),
        Some("foo/bar\"baz/test.rs")
    );
}

#[test]
fn extract_streaming_content_progressively() {
    let chunk1 = r#"{"path": "test.txt", "content": "first line\n"#;
    assert_eq!(extract_json_streaming_content(chunk1).as_deref(), Some("first line\n"));

    let chunk2 = r#"{"path": "test.txt", "content": "first line\nsecond line"#;
    assert_eq!(
        extract_json_streaming_content(chunk2).as_deref(),
        Some("first line\nsecond line")
    );

    let chunk3 = r#"{"path": "test.txt", "content": "first line\nsecond line\n"}"#;
    assert_eq!(
        extract_json_streaming_content(chunk3).as_deref(),
        Some("first line\nsecond line\n")
    );
}

#[test]
fn extract_handles_escapes() {
    let json = r#"{"content": "tab:\t, quote:\""}"#;
    assert_eq!(
        extract_json_streaming_content(json).as_deref(),
        Some("tab:\t, quote:\"")
    );
}
