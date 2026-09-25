use super::*;
use crate::engine::runner::sink::{TerminalApprovalSink, TerminalSinkConfig};
use rho_harness_core::session::SessionManager;
use std::sync::Arc;

fn mock_sink() -> Arc<TerminalApprovalSink> {
    let temp_dir = std::env::temp_dir().join(format!("streaming_test_{}", uuid::Uuid::new_v4()));
    let session = SessionManager::new(&temp_dir, None).unwrap();
    TerminalApprovalSink::new(
        &crate::engine::eval::presenter::presenter(),
        TerminalSinkConfig {
            model_label: "test-model".to_string(),
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        session,
    )
}

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
fn extract_handles_all_escapes() {
    let json = r#"{"content": "quote:\" slash:\\ slash2:\/ backspace:\b ff:\f lf:\n cr:\r tab:\t unknown:\a"}"#;
    assert_eq!(
        extract_json_streaming_content(json).as_deref(),
        Some("quote:\" slash:\\ slash2:/ backspace:\x08 ff:\x0c lf:\n cr:\r tab:\t unknown:\\a")
    );
}

#[test]
fn extract_trailing_escape_in_streaming_content() {
    let json = "{\"content\": \"hello\\";
    assert_eq!(extract_json_streaming_content(json).as_deref(), Some("hello"));

    let non_streaming = "{\"path\": \"hello\\";
    assert_eq!(extract_json_string_field(non_streaming, "path"), None);
}

#[test]
fn extract_missing_or_malformed_fields() {
    assert_eq!(extract_json_string_field("{}", "path"), None);
    assert_eq!(extract_json_string_field("{\"path\"", "path"), None);
    assert_eq!(extract_json_string_field("{\"path\": 123}", "path"), None);

    assert_eq!(extract_json_streaming_content("{}"), None);
    assert_eq!(extract_json_streaming_content("{\"content\""), None);
    assert_eq!(extract_json_streaming_content("{\"content\": false}"), None);
}

#[test]
fn streaming_tool_tracker_write_lifecycle() {
    let sink = mock_sink();
    let mut tracker = StreamingToolTracker::default();

    tracker.handle_delta(ToolCallDeltaContent::Name("write".to_string()), &sink);
    tracker.handle_delta(
        ToolCallDeltaContent::Delta(r#"{"path": "foo.txt", "content": "hello "#.to_string()),
        &sink,
    );
    assert!(tracker.path_started);
    assert_eq!(tracker.streamed_content_len, 6);

    // Second chunk with additional content
    tracker.handle_delta(ToolCallDeltaContent::Delta(r#"world"}"#.to_string()), &sink);
    assert_eq!(tracker.streamed_content_len, 11);

    // Third chunk with no new content length
    tracker.handle_delta(ToolCallDeltaContent::Delta("".to_string()), &sink);
    assert_eq!(tracker.streamed_content_len, 11);

    // Reset clears state
    tracker.reset();
    assert!(!tracker.path_started);
    assert_eq!(tracker.streamed_content_len, 0);
    assert!(tracker.name.is_none());
}

#[test]
fn streaming_tool_tracker_file_path_fallback() {
    let sink = mock_sink();
    let mut tracker = StreamingToolTracker::default();

    tracker.handle_delta(ToolCallDeltaContent::Name("write".to_string()), &sink);
    tracker.handle_delta(
        ToolCallDeltaContent::Delta(r#"{"file_path": "bar.txt", "content": "data"}"#.to_string()),
        &sink,
    );
    assert!(tracker.path_started);
    assert_eq!(tracker.streamed_content_len, 4);
}

#[test]
fn streaming_tool_tracker_non_write_tool_ignored() {
    let sink = mock_sink();
    let mut tracker = StreamingToolTracker::default();

    tracker.handle_delta(ToolCallDeltaContent::Name("bash".to_string()), &sink);
    tracker.handle_delta(
        ToolCallDeltaContent::Delta(r#"{"command": "echo hi"}"#.to_string()),
        &sink,
    );
    assert!(!tracker.path_started);
    assert_eq!(tracker.streamed_content_len, 0);
}

#[test]
fn streaming_tool_tracker_partial_without_path() {
    let sink = mock_sink();
    let mut tracker = StreamingToolTracker::default();

    tracker.handle_delta(ToolCallDeltaContent::Name("write".to_string()), &sink);
    tracker.handle_delta(
        ToolCallDeltaContent::Delta(r#"{"no_path_key": 123}"#.to_string()),
        &sink,
    );
    assert!(!tracker.path_started);
}
