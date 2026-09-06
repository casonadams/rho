use super::{find, fixture};
use crate::tools::fd::entry::format_results;
use crate::tools::fd::{FdArgs, FdEntry, FdFormat, FdTool};
use crate::tools::types::generated_schema;

#[tokio::test]
async fn limit_truncates_with_a_narrowing_notice() {
    let dir = fixture();
    let result = find(&dir, ".", |args| args.limit = Some(2)).await;
    assert!(!result.is_error);
    assert_eq!(
        result.content,
        "README.md\nsrc\n\n[showing first 2 of 6 matches; narrow with a tighter pattern, path, or type]"
    );
}

#[tokio::test]
async fn limit_clamps_to_at_least_one() {
    let dir = fixture();
    let result = find(&dir, ".", |args| args.limit = Some(0)).await;
    assert_eq!(
        result.content,
        "README.md\n\n[showing first 1 of 6 matches; narrow with a tighter pattern, path, or type]"
    );
}

#[test]
fn oversized_output_is_byte_capped_before_the_notices() {
    // format_results is pure, so synthetic long paths stand in for a
    // thousand-file fixture; 300 rows of 237 chars exceed the 50KB cap.
    let paths: Vec<String> = (0..300).map(|i| format!("dir{i:0>3}/{}", "p".repeat(230))).collect();
    let entries: Vec<FdEntry> = paths
        .into_iter()
        .map(|relative| FdEntry {
            relative,
            is_dir: false,
            stats: None,
        })
        .collect();
    let result = format_results(
        entries,
        FdFormat {
            hit_ceiling: false,
            limit: 250,
            show_stats: false,
        },
    );
    let body = result.content.split("\n\n").next().unwrap();
    assert!(body.len() <= crate::tools::truncate::DEFAULT_MAX_BYTES);
    assert!(result.content.contains("showing first 250 of 300 matches"));
    assert!(result.content.ends_with("path, or type. 50.0KB limit reached]"));
}

#[tokio::test]
async fn path_without_pattern_lists_files_in_subtree() {
    let dir = fixture();
    let args = FdArgs {
        path: Some("src".to_string()),
        ..Default::default()
    };
    let result = FdTool::new(dir.path()).execute(args).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("src/main.rs"));
    assert!(result.content.contains("src/lib.rs"));
    assert!(result.content.contains("src/ui/widget.rs"));
    assert!(!result.content.contains("README.md"));
}

#[tokio::test]
async fn invalid_regex_names_the_pattern() {
    let dir = fixture();
    let result = find(&dir, "(", |_| {}).await;
    assert!(result.is_error);
    assert!(result.content.contains("invalid pattern"));
    assert!(result.content.contains("\"(\""));
}

#[tokio::test]
async fn path_outside_workspace_is_searched_successfully() {
    let dir = fixture();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("external.txt"), "external").unwrap();

    let outside_path = outside.path().to_str().unwrap().to_string();
    let result = find(&dir, "external", |args| args.path = Some(outside_path)).await;
    assert!(!result.is_error);
    assert!(result.content.contains("external.txt"));
}

#[tokio::test]
async fn missing_path_errors_without_panicking() {
    let dir = fixture();
    let result = find(&dir, ".", |args| args.path = Some("does/not/exist".to_string())).await;
    assert!(result.is_error);
    assert!(result.content.contains("path not found"));
}

#[tokio::test]
async fn path_scopes_results_to_a_subtree() {
    let dir = fixture();
    let result = find(&dir, "widget", |args| args.path = Some("src".to_string())).await;
    assert!(!result.is_error);
    assert_eq!(result.content, "src/ui/widget.rs");
    assert!(!result.content.contains("README.md"));
}

#[test]
fn schema_exposes_renamed_type_property() {
    let schema = generated_schema::<FdArgs>();
    for prop in ["type", "pattern", "stats", "min_lines", "max_lines", "sort"] {
        assert!(schema["properties"].get(prop).is_some());
    }
    assert!(schema["properties"].get("file_type").is_none());
    let required = schema.get("required").and_then(|r| r.as_array());
    assert!(
        required.is_none() || !required.unwrap().iter().any(|v| v == "pattern"),
        "pattern should be optional in schema"
    );
}

#[tokio::test]
async fn fd_cancellation_halts_traversal_immediately() {
    let dir = fixture();
    let cancellation = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let query = super::query::FdQuery {
        workspace_root: dir.path().to_path_buf(),
        search_root: dir.path().to_path_buf(),
        search_path_display: None,
        regex: None,
        types: None,
        include_hidden: false,
        depth: None,
        stats_needed: false,
        min_lines: None,
        max_lines: None,
        sort: None,
        show_stats: false,
        timeout: None,
        cancellation: Some(cancellation),
    };
    let result = query.run(10);
    assert!(!result.is_error);
    assert_eq!(result.content, "No files found matching pattern");
}

#[tokio::test]
async fn fd_timeout_returns_timeout_error() {
    let dir = fixture();
    let query = super::query::FdQuery {
        workspace_root: dir.path().to_path_buf(),
        search_root: dir.path().to_path_buf(),
        search_path_display: None,
        regex: None,
        types: None,
        include_hidden: false,
        depth: None,
        stats_needed: false,
        min_lines: None,
        max_lines: None,
        sort: None,
        show_stats: false,
        timeout: Some(std::time::Duration::from_millis(0)),
        cancellation: None,
    };
    let result = query.run(10);
    assert!(result.is_error);
    assert!(result.content.contains("Search timed out"));
}
