use super::{fixture, search};
use crate::tools::rg::RgArgs;
use crate::tools::truncate::DEFAULT_MAX_BYTES;
use crate::tools::types::generated_schema;

#[tokio::test]
async fn long_match_lines_are_truncated_with_a_marked_suffix_and_notice() {
    let dir = fixture();
    std::fs::write(dir.path().join("long.txt"), format!("needle{}\n", "x".repeat(600))).unwrap();
    let result = search(&dir, "needle", |_| {}).await;
    assert!(!result.is_error);
    let text = result.content.strip_prefix("long.txt:1: ").unwrap();
    let line_text = text.split("\n\n").next().unwrap();
    assert_eq!(line_text.chars().count(), 500 + "... [truncated]".len());
    assert!(line_text.ends_with("... [truncated]"));
    assert!(
        result
            .content
            .contains("\n\n[Some lines truncated to 500 chars. Use read tool to see full lines]")
    );
}

#[tokio::test]
async fn line_truncation_notice_tracks_only_shown_matches() {
    let dir = fixture();
    let mut content = String::new();
    for i in 0..10 {
        content.push_str(&format!("needle line {i}\n"));
    }
    for _ in 10..30 {
        content.push_str(&format!("needle{}\n", "y".repeat(600)));
    }
    std::fs::write(dir.path().join("lines.txt"), content).unwrap();
    let result = search(&dir, "needle", |args| args.limit = Some(10)).await;
    assert!(!result.is_error);
    // Long lines exist beyond the shown window but must not claim the notice.
    assert!(!result.content.contains("Some lines truncated"));
    assert!(result.content.contains("showing first 10 of 30"));
}

#[tokio::test]
async fn limit_truncates_with_a_narrowing_notice() {
    let dir = fixture();
    let result = search(&dir, ".", |args| args.limit = Some(2)).await;
    assert!(!result.is_error);
    assert_eq!(
        result.content,
        "README.md:1: # Fixture\nREADME.md:2: todo list\n\n[showing first 2 of 7 matches; narrow with a tighter pattern, path, or type]"
    );
}

#[tokio::test]
async fn limit_clamps_to_at_least_one() {
    let dir = fixture();
    let result = search(&dir, ".", |args| args.limit = Some(0)).await;
    assert_eq!(
        result.content,
        "README.md:1: # Fixture\n\n[showing first 1 of 7 matches; narrow with a tighter pattern, path, or type]"
    );
}

#[tokio::test]
async fn collection_ceiling_stops_traversal_and_flags_the_notice() {
    let dir = fixture();
    let many: String = (0..6000).map(|i| format!("needle line {i}\n")).collect();
    std::fs::write(dir.path().join("many.txt"), many).unwrap();
    let result = search(&dir, "needle", |_| {}).await;
    assert!(!result.is_error);
    let lines: Vec<&str> = result.content.lines().collect();
    assert_eq!(lines.len(), 202);
    assert_eq!(lines[0], "many.txt:1: needle line 0");
    assert_eq!(
        lines[201],
        "[showing first 200 of 5000+ matches (collection ceiling reached); narrow with a tighter pattern, path, or type]"
    );
}

#[tokio::test]
async fn oversized_output_is_byte_capped_before_the_notices() {
    let dir = fixture();
    let row = format!("needle{}\n", "x".repeat(300));
    let many: String = std::iter::repeat_n(row, 400).collect();
    std::fs::write(dir.path().join("many.txt"), many).unwrap();
    let result = search(&dir, "needle", |_| {}).await;
    assert!(!result.is_error);
    assert!(result.content.contains("showing first 200 of 400"));
    assert!(result.content.contains("50.0KB limit reached"));
    // Byte truncation applies to the rendered rows; notices ride behind the
    // truncated block on their own paragraph.
    let body = result.content.split("\n\n").next().unwrap();
    assert!(body.len() <= DEFAULT_MAX_BYTES);
    assert!(body.lines().count() < 200);
}

#[tokio::test]
async fn empty_pattern_is_a_tool_error() {
    let dir = fixture();
    let result = search(&dir, "   ", |_| {}).await;
    assert!(result.is_error);
    assert!(result.content.contains("Empty pattern"));
}

#[tokio::test]
async fn invalid_regex_names_the_pattern() {
    let dir = fixture();
    let result = search(&dir, "(", |_| {}).await;
    assert!(result.is_error);
    assert!(result.content.contains("invalid pattern"));
    assert!(result.content.contains("\"(\""));
}

#[tokio::test]
async fn path_outside_the_workspace_errors() {
    let dir = fixture();
    let result = search(&dir, "needle", |args| args.path = Some("../elsewhere".to_string())).await;
    assert!(result.is_error);
    assert!(result.content.contains("outside the workspace"));
}

#[tokio::test]
async fn missing_path_errors_without_panicking() {
    let dir = fixture();
    let result = search(&dir, "needle", |args| args.path = Some("does/not/exist".to_string())).await;
    assert!(result.is_error);
    assert!(result.content.contains("path not found"));
}

#[tokio::test]
async fn path_scopes_results_to_a_subtree() {
    let dir = fixture();
    let result = search(&dir, "pub", |args| args.path = Some("src".to_string())).await;
    assert!(!result.is_error);
    assert_eq!(result.content.lines().count(), 3);
    assert!(!result.content.contains("README.md"));
}

#[test]
fn schema_exposes_renamed_type_property() {
    let schema = generated_schema::<RgArgs>();
    assert!(schema["properties"].get("type").is_some());
    assert!(schema["properties"].get("file_type").is_none());
    assert!(schema["properties"].get("pattern").is_some());
}
