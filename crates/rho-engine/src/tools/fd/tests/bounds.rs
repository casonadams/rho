use super::{find, fixture};
use crate::test_support::{assert_invalid_pattern, assert_timeout_error, outside_workspace_file};
use crate::tools::fd::entry::format_results;
use crate::tools::fd::entry::{FdEntry, FdFormat};
use crate::tools::fd::{FdArgs, FdTool};
use crate::tools::listing::{Collected, PageRequest};
use crate::tools::types::generated_schema;
use rho_test_support::TestDir;

fn file(relative: String) -> FdEntry {
    FdEntry {
        relative,
        is_dir: false,
        stats: None,
        size: None,
        source: None,
    }
}

fn dir_entry(relative: &str) -> FdEntry {
    FdEntry {
        is_dir: true,
        ..file(relative.to_string())
    }
}

fn collected(items: Vec<FdEntry>) -> Collected<FdEntry> {
    let seen = items.len();
    Collected { items, seen }
}

#[tokio::test]
async fn directories_with_listed_entries_appear_only_as_their_header() {
    let dir = fixture();
    let result = find(&dir, ".", |_| {}).await;
    assert_eq!(
        result.content,
        "4 paths\n\n./\n  README.md\nsrc/\n  lib.rs\n  main.rs\nsrc/ui/\n  widget.rs"
    );
}

#[tokio::test]
async fn directories_without_listed_entries_stay_listed() {
    let dir = fixture();
    std::fs::create_dir(dir.join("empty")).unwrap();
    let result = find(&dir, "empty|ui", |_| {}).await;
    assert_eq!(result.content, "2 paths\n\n./\n  empty/\nsrc/ui/\n  widget.rs");
}

#[tokio::test]
async fn limit_pages_with_the_next_offset() {
    let dir = fixture();
    let result = find(&dir, ".", |args| args.limit = Some(2)).await;
    assert!(!result.is_error);
    assert_eq!(
        result.content,
        "4 paths\n\n./\n  README.md\nsrc/\n  lib.rs\n\n[showing paths 1-2 of 4; next page: offset=2]"
    );
}

#[tokio::test]
async fn offset_continues_and_repeats_the_directory_header() {
    let dir = fixture();
    let result = find(&dir, ".", |args| {
        args.offset = Some(2);
        args.limit = Some(1);
    })
    .await;
    assert_eq!(
        result.content,
        "4 paths\n\nsrc/\n  main.rs\n\n[showing paths 3-3 of 4; next page: offset=3]"
    );
}

#[tokio::test]
async fn limit_clamps_to_at_least_one() {
    let dir = fixture();
    let result = find(&dir, ".", |args| args.limit = Some(0)).await;
    assert_eq!(
        result.content,
        "4 paths\n\n./\n  README.md\n\n[showing paths 1-1 of 4; next page: offset=1]"
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
        },
    );
    let body = result.content.split("\n\n").nth(1).unwrap();
    assert!(body.len() <= crate::tools::listing::LISTING_PAGE_BYTES);
    let shown = body.lines().count() / 2;
    assert!(
        result
            .content
            .ends_with(&format!("[showing paths 1-{shown} of 300; next page: offset={shown}]"))
    );
}

#[tokio::test]
async fn path_without_pattern_lists_files_in_subtree() {
    let dir = fixture();
    let args = FdArgs {
        path: Some("src".to_string()),
        ..Default::default()
    };
    let result = FdTool::new(&dir).execute(args).await.unwrap();
    assert_eq!(
        result.content,
        "3 paths\n\nsrc/\n  lib.rs\n  main.rs\nsrc/ui/\n  widget.rs"
    );
}

#[test]
fn stats_table_keeps_directories_that_have_listed_entries() {
    let entries = vec![dir_entry("src"), file("src/lib.rs".to_string())];
    let result = format_results(
        collected(entries),
        FdFormat {
            request: PageRequest::default(),
            show_stats: true,
        },
    );
    assert!(result.content.starts_with("2 paths\n\n"), "{}", result.content);
    assert!(result.content.contains("  src/\n"), "{}", result.content);
}

#[tokio::test]
async fn depth_counts_from_the_search_path_without_listing_it() {
    let dir = fixture();
    let result = find(&dir, ".", |args| {
        args.path = Some("src".to_string());
        args.depth = Some(1);
    })
    .await;
    assert_eq!(result.content, "3 paths\n\nsrc/\n  lib.rs\n  main.rs\n  ui/");
}

#[tokio::test]
async fn file_path_lists_that_file() {
    let dir = fixture();
    let result = find(&dir, ".", |args| args.path = Some("src/lib.rs".to_string())).await;
    assert_eq!(result.content, "1 path\n\nsrc/\n  lib.rs");
}

#[tokio::test]
async fn invalid_regex_names_the_pattern() {
    let dir = fixture();
    let result = find(&dir, "(", |_| {}).await;
    assert_invalid_pattern(&result, "(");
}

#[tokio::test]
async fn path_outside_workspace_is_searched_successfully() {
    let dir = fixture();
    let outside = outside_workspace_file("external.txt", "external");
    let outside_path = outside.to_str().unwrap().to_string();
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

fn query(dir: &TestDir) -> super::query::FdQuery {
    super::query::FdQuery {
        workspace_root: dir.to_path_buf(),
        search_root: dir.to_path_buf(),
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
    }
}

#[tokio::test]
async fn fd_cancellation_halts_traversal_immediately() {
    let dir = fixture();
    let mut cancelled = query(&dir);
    cancelled.cancellation = Some(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)));
    let result = cancelled.run(PageRequest::default());
    assert!(!result.is_error);
    assert_eq!(result.content, "No paths found");
}

#[tokio::test]
async fn fd_timeout_returns_timeout_error() {
    let dir = fixture();
    let mut timed = query(&dir);
    timed.timeout = Some(std::time::Duration::from_millis(0));
    let result = timed.run(PageRequest::default());
    assert_timeout_error(&result);
}
