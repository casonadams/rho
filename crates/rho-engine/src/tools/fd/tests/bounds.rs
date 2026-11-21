use super::{find, fixture};
use crate::tools::fd::entry::format_results;
use crate::tools::fd::{FdArgs, FdEntry, FdFormat, FdTool};
use crate::tools::listing::{Collected, PageRequest};
use crate::tools::types::generated_schema;

fn file(relative: String) -> FdEntry {
    FdEntry {
        relative,
        is_dir: false,
        stats: None,
        size: None,
        source: None,
    }
}

fn collected(items: Vec<FdEntry>) -> Collected<FdEntry> {
    let seen = items.len();
    Collected { items, seen }
}

#[tokio::test]
async fn listing_is_grouped_by_parent_directory() {
    let dir = fixture();
    let result = find(&dir, ".", |_| {}).await;
    assert_eq!(
        result.content,
        "6 paths\n\n./\n  README.md\n  src/\nsrc/\n  lib.rs\n  main.rs\n  ui/\nsrc/ui/\n  widget.rs"
    );
}

#[tokio::test]
async fn limit_pages_with_the_next_offset() {
    let dir = fixture();
    let result = find(&dir, ".", |args| args.limit = Some(2)).await;
    assert!(!result.is_error);
    assert_eq!(
        result.content,
        "6 paths\n\n./\n  README.md\n  src/\n\n[showing paths 1-2 of 6; next page: offset=2]"
    );
}

#[tokio::test]
async fn offset_continues_and_repeats_the_directory_header() {
    let dir = fixture();
    let result = find(&dir, ".", |args| {
        args.offset = Some(3);
        args.limit = Some(2);
    })
    .await;
    assert_eq!(
        result.content,
        "6 paths\n\nsrc/\n  main.rs\n  ui/\n\n[showing paths 4-5 of 6; next page: offset=5]"
    );
}

#[tokio::test]
async fn limit_clamps_to_at_least_one() {
    let dir = fixture();
    let result = find(&dir, ".", |args| args.limit = Some(0)).await;
    assert_eq!(
        result.content,
        "6 paths\n\n./\n  README.md\n\n[showing paths 1-1 of 6; next page: offset=1]"
    );
}

#[test]
fn oversized_output_fills_the_byte_budget_and_names_the_next_offset() {
    let entries = (0..300)
        .map(|i| file(format!("dir{i:0>3}/{}", "p".repeat(230))))
        .collect();
    let result = format_results(
        collected(entries),
        FdFormat {
            request: PageRequest::default(),
            show_stats: false,
            artifact_dir: None,
        },
    );
    let body = result.content.split("\n\n").nth(1).unwrap();
    assert!(body.len() <= crate::tools::truncate::DEFAULT_MAX_BYTES);
    let shown = body.lines().count() / 2;
    assert!(
        result
            .content
            .ends_with(&format!("[showing paths 1-{shown} of 300; next page: offset={shown}]"))
    );
}

#[tokio::test]
async fn test_fd_oversized_search_spills_artifact() {
    let dir = tempfile::tempdir().unwrap();
    let entries = (0..500)
        .map(|i| {
            file(format!(
                "dir_{i:04}/very_long_path_name_{i:04}/{}",
                "nested_".repeat(20)
            ))
        })
        .collect();
    let result = format_results(
        collected(entries),
        FdFormat {
            request: PageRequest::default(),
            show_stats: false,
            artifact_dir: Some(dir.path().join(".rho/artifacts")),
        },
    );
    assert!(!result.is_error);
    assert!(result.content.contains(".rho/artifacts"));
    assert!(result.content.contains("[Output truncated. Full content"));

    let artifact_dir = dir.path().join(".rho/artifacts");
    let files: Vec<_> = std::fs::read_dir(&artifact_dir).unwrap().collect();
    assert_eq!(files.len(), 1);
    let spilled_content = std::fs::read_to_string(files[0].as_ref().unwrap().path()).unwrap();
    assert!(spilled_content.contains("dir_0000"));
    assert!(spilled_content.contains("dir_0499"));
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
    assert!(result.content.contains("src/\n  lib.rs\n  main.rs"));
    assert!(result.content.contains("src/ui/\n  widget.rs"));
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
    assert!(result.content.contains("  external.txt"));
}

#[tokio::test]
async fn missing_path_errors_without_panicking() {
    let dir = fixture();
    let result = find(&dir, ".", |args| args.path = Some("does/not/exist".to_string())).await;
    assert!(result.is_error);
    assert!(result.content.contains("path not found"));
    let typo = find(&dir, ".", |args| args.path = Some("src/widget.rs".to_string())).await;
    assert!(
        typo.content.ends_with("Did you mean: src/ui/widget.rs?"),
        "{}",
        typo.content
    );
}

#[tokio::test]
async fn path_scopes_results_to_a_subtree() {
    let dir = fixture();
    let result = find(&dir, "widget", |args| args.path = Some("src".to_string())).await;
    assert!(!result.is_error);
    assert_eq!(result.content, "1 path\n\nsrc/ui/\n  widget.rs");
}

#[test]
fn schema_exposes_renamed_type_property() {
    let schema = generated_schema::<FdArgs>();
    for prop in [
        "type",
        "pattern",
        "stats",
        "min_lines",
        "max_lines",
        "sort",
        "offset",
        "limit",
    ] {
        assert!(schema["properties"].get(prop).is_some(), "{prop}");
    }
    assert!(schema["properties"].get("file_type").is_none());
    let required = schema.get("required").and_then(|r| r.as_array());
    assert!(
        required.is_none() || !required.unwrap().iter().any(|v| v == "pattern"),
        "pattern should be optional in schema"
    );
}

fn query(dir: &tempfile::TempDir) -> super::query::FdQuery {
    super::query::FdQuery {
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
        cancellation: None,
        artifact_dir: None,
    }
}

#[tokio::test]
async fn fd_cancellation_halts_traversal_immediately() {
    let dir = fixture();
    let mut cancelled = query(&dir);
    cancelled.cancellation = Some(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)));
    let result = cancelled.run(PageRequest::default());
    assert!(!result.is_error);
    assert_eq!(result.content, "No files found matching pattern");
}

#[tokio::test]
async fn fd_timeout_returns_timeout_error() {
    let dir = fixture();
    let mut timed = query(&dir);
    timed.timeout = Some(std::time::Duration::from_millis(0));
    let result = timed.run(PageRequest::default());
    assert!(result.is_error);
    assert!(result.content.contains("Search timed out"));
}
