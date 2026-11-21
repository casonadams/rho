use super::{fixture, search};
use crate::tools::listing::LISTING_PAGE_BYTES;
use crate::tools::listing::PageRequest;
use crate::tools::rg::{RG_COLLECTION_CEILING, RgArgs, RgOutputMode};
use crate::tools::types::generated_schema;

const MODE_HINT: &str = "output_mode 'files' or 'count' covers every matching file in far fewer bytes";

#[tokio::test]
async fn long_match_lines_are_truncated_with_a_marked_suffix_and_notice() {
    let dir = fixture();
    std::fs::write(dir.path().join("long.txt"), format!("needle{}\n", "x".repeat(600))).unwrap();
    let result = search(&dir, "needle", |_| {}).await;
    assert!(!result.is_error);
    let text = result
        .content
        .strip_prefix("1 match in 1 file\n\nlong.txt\n1:")
        .unwrap();
    let line_text = text.split("\n\n").next().unwrap();
    assert_eq!(line_text.chars().count(), 500 + "... [truncated]".len());
    assert!(line_text.ends_with("... [truncated]"));
    assert!(
        result
            .content
            .ends_with("\n\n[Some lines truncated to 500 chars; use read with offset to see full lines]")
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
    assert!(!result.content.contains("Some lines truncated"));
    assert!(
        result
            .content
            .contains("[showing matches 1-10 of 30; next page: offset=10;")
    );
}

#[tokio::test]
async fn limit_pages_with_the_next_offset() {
    let dir = fixture();
    let result = search(&dir, ".", |args| args.limit = Some(2)).await;
    assert!(!result.is_error);
    assert_eq!(
        result.content,
        format!(
            "7 matches in 4 files\n\nREADME.md\n1:# Fixture\n2:todo list\n\n[showing matches 1-2 of 7; next page: offset=2; {MODE_HINT}]"
        )
    );
}

#[tokio::test]
async fn offset_continues_where_the_previous_page_ended() {
    let dir = fixture();
    let result = search(&dir, ".", |args| {
        args.offset = Some(2);
        args.limit = Some(2);
    })
    .await;
    assert_eq!(
        result.content,
        "7 matches in 4 files\n\nREADME.md\n3:pub markdown\n\nsrc/lib.rs\n1:pub mod ui;\n\n[showing matches 3-4 of 7; next page: offset=4]"
    );
}

#[tokio::test]
async fn limit_clamps_to_at_least_one() {
    let dir = fixture();
    let result = search(&dir, ".", |args| args.limit = Some(0)).await;
    assert!(
        result
            .content
            .starts_with("7 matches in 4 files\n\nREADME.md\n1:# Fixture\n\n[showing matches 1-1 of 7;")
    );
}

#[tokio::test]
async fn every_match_is_reachable_by_offset_with_exact_totals() {
    let dir = fixture();
    let many: String = (0..6000).map(|i| format!("needle line {i}\n")).collect();
    std::fs::write(dir.path().join("many.txt"), many).unwrap();
    let result = search(&dir, "needle", |args| args.offset = Some(5998)).await;
    assert_eq!(
        result.content,
        "6000 matches in 1 file\n\nmany.txt\n5999:needle line 5998\n6000:needle line 5999\n\n[showing matches 5999-6000 of 6000]"
    );
}

#[tokio::test]
async fn collection_ceiling_keeps_the_first_matches_in_path_order() {
    let dir = fixture();
    let rows = |prefix: &str, n: usize| -> String { (0..n).map(|i| format!("{prefix} needle {i}\n")).collect() };
    std::fs::write(dir.path().join("b.txt"), rows("b", RG_COLLECTION_CEILING)).unwrap();
    std::fs::write(dir.path().join("a.txt"), rows("a", 5)).unwrap();
    let result = search(&dir, "needle", |args| args.offset = Some(RG_COLLECTION_CEILING - 1)).await;
    let total = RG_COLLECTION_CEILING + 5;
    assert!(
        result
            .content
            .starts_with(&format!("{total} matches in 2 files\n\nb.txt\n"))
    );
    assert!(result.content.contains(&format!(
        "\n{}:b needle {}\n",
        RG_COLLECTION_CEILING - 5,
        RG_COLLECTION_CEILING - 6
    )));
    assert!(
        result
            .content
            .contains("only the first 20000 matches in sort order can be paged")
    );
}

#[tokio::test]
async fn offset_past_the_end_reports_the_valid_range() {
    let dir = fixture();
    let result = search(&dir, "pub", |args| args.offset = Some(10)).await;
    assert_eq!(
        result.content,
        "4 matches in 3 files\n\n[offset=10 is past the end; valid offsets are 0-3]"
    );
}

#[tokio::test]
async fn files_mode_lists_matching_files_grouped_by_directory() {
    let dir = fixture();
    let result = search(&dir, "pub", |args| args.output_mode = Some(RgOutputMode::Files)).await;
    assert_eq!(
        result.content,
        "3 files with matches\n\n./\n  README.md\nsrc/\n  lib.rs\nsrc/ui/\n  widget.rs"
    );
}

#[tokio::test]
async fn count_mode_reports_matches_per_file() {
    let dir = fixture();
    let result = search(&dir, "pub", |args| args.output_mode = Some(RgOutputMode::Count)).await;
    assert_eq!(
        result.content,
        "4 matches in 3 files\n\n./\n  README.md:1\nsrc/\n  lib.rs:1\nsrc/ui/\n  widget.rs:2"
    );
}

#[tokio::test]
async fn oversized_output_fills_the_byte_budget_and_names_the_next_offset() {
    let dir = fixture();
    let row = format!("needle{}\n", "x".repeat(400));
    let many: String = std::iter::repeat_n(row, 400).collect();
    std::fs::write(dir.path().join("many.txt"), many).unwrap();
    let result = search(&dir, "needle", |_| {}).await;
    assert!(!result.is_error);
    let body = result.content.split("\n\n").nth(1).unwrap();
    let row_bytes = "1:needle".len() + 400 + 1;
    assert!(body.len() <= LISTING_PAGE_BYTES);
    assert!(
        body.len() + row_bytes > LISTING_PAGE_BYTES,
        "the page is filled: {}",
        body.len()
    );
    let shown = body.lines().count() - 1;
    assert!(result.content.contains(&format!(
        "[showing matches 1-{shown} of 400; next page: offset={shown};"
    )));
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
async fn path_outside_workspace_is_searched_successfully() {
    let dir = fixture();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("external.txt"), "external target line\n").unwrap();

    let outside_path = outside.path().to_str().unwrap().to_string();
    let result = search(&dir, "external target", |args| args.path = Some(outside_path)).await;
    assert!(!result.is_error);
    assert!(result.content.contains("external.txt\n1:external target line"));
}

#[tokio::test]
async fn missing_path_errors_without_panicking() {
    let dir = fixture();
    let result = search(&dir, "needle", |args| args.path = Some("does/not/exist".to_string())).await;
    assert!(result.is_error);
    assert!(result.content.contains("path not found"));
    assert!(!result.content.contains("Did you mean"), "{}", result.content);
    let typo = search(&dir, "needle", |args| args.path = Some("src/iu".to_string())).await;
    assert!(
        typo.content.ends_with("path not found: src/iu\nDid you mean: src/ui?"),
        "{}",
        typo.content
    );
}

#[tokio::test]
async fn path_scopes_results_to_a_subtree() {
    let dir = fixture();
    let result = search(&dir, "pub", |args| args.path = Some("src".to_string())).await;
    assert!(!result.is_error);
    assert!(result.content.starts_with("3 matches in 2 files\n"));
    assert!(!result.content.contains("README.md"));
}

#[test]
fn schema_exposes_renamed_type_property_and_paging() {
    let schema = generated_schema::<RgArgs>();
    for prop in ["type", "pattern", "output_mode", "offset", "limit"] {
        assert!(schema["properties"].get(prop).is_some(), "{prop}");
    }
    assert!(schema["properties"].get("file_type").is_none());
}

fn query(dir: &tempfile::TempDir) -> super::super::RgQuery {
    super::super::RgQuery {
        workspace_root: dir.path().to_path_buf(),
        search_root: dir.path().to_path_buf(),
        search_path_display: None,
        matcher: grep_regex::RegexMatcher::new("pub").unwrap(),
        types: None,
        include_hidden: false,
        mode: RgOutputMode::Content,
        timeout: None,
        cancellation: None,
    }
}

#[tokio::test]
async fn rg_cancellation_halts_traversal_immediately() {
    let dir = fixture();
    let mut cancelled = query(&dir);
    cancelled.cancellation = Some(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)));
    let result = cancelled.run(PageRequest::default());
    assert!(!result.is_error);
    assert_eq!(result.content, "No matches found");
}

#[tokio::test]
async fn rg_timeout_returns_timeout_error() {
    let dir = fixture();
    let mut timed = query(&dir);
    timed.timeout = Some(std::time::Duration::from_millis(0));
    let result = timed.run(PageRequest::default());
    assert!(result.is_error);
    assert!(result.content.contains("Search timed out"));
}
