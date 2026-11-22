use super::{fixture, search};
use crate::tools::rg::RgOutputMode;

#[tokio::test]
async fn matches_report_path_line_and_text() {
    let dir = fixture();
    let result = search(&dir, "widget", |_| {}).await;
    assert!(!result.is_error);
    assert_eq!(
        result.content,
        "1 match in 1 file\n\nsrc/ui/widget.rs\n1:pub struct Widget;"
    );
}

#[tokio::test]
async fn results_are_grouped_by_file_and_ordered_by_path_then_line() {
    let dir = fixture();
    let result = search(&dir, "pub", |_| {}).await;
    assert_eq!(
        result.content.lines().collect::<Vec<_>>(),
        [
            "4 matches in 3 files",
            "",
            "README.md",
            "3:pub markdown",
            "",
            "src/lib.rs",
            "1:pub mod ui;",
            "",
            "src/ui/widget.rs",
            "1:pub struct Widget;",
            "2:pub enum Kind { A, B }",
        ]
    );
}

#[tokio::test]
async fn pattern_is_smart_case() {
    let dir = fixture();
    let lower = search(&dir, "todo", |_| {}).await;
    assert_eq!(lower.content, "1 match in 1 file\n\nREADME.md\n2:todo list");

    let upper = search(&dir, "TODO", |_| {}).await;
    assert_eq!(upper.content, "No matches found");

    let mixed = search(&dir, "Todo", |_| {}).await;
    assert_eq!(mixed.content, "No matches found");
}

#[tokio::test]
async fn context_shows_surrounding_lines_and_separates_detached_blocks() {
    let dir = fixture();
    let body = "a\nneedle one\nb\nc\nd\ne\nf\nneedle two\ng\nneedle three\nh\n";
    std::fs::write(dir.path().join("ctx.txt"), body).unwrap();
    let result = search(&dir, "needle", |args| args.context = Some(1)).await;
    assert_eq!(
        result.content.lines().collect::<Vec<_>>(),
        [
            "3 matches in 1 file",
            "",
            "ctx.txt",
            "1-a",
            "2:needle one",
            "3-b",
            "--",
            "7-f",
            "8:needle two",
            "9-g",
            "10:needle three",
            "11-h",
        ]
    );
}

#[tokio::test]
async fn context_is_capped_and_ignored_outside_content_mode() {
    let dir = fixture();
    let body = format!("{}needle\n", "x\n".repeat(30));
    std::fs::write(dir.path().join("deep.txt"), body).unwrap();
    let capped = search(&dir, "needle", |args| args.context = Some(50)).await;
    assert!(capped.content.contains("\n21-x\n"), "{}", capped.content);
    assert!(!capped.content.contains("\n20-x\n"), "{}", capped.content);

    let count = search(&dir, "needle", |args| {
        args.context = Some(3);
        args.output_mode = Some(RgOutputMode::Count);
    })
    .await;
    assert_eq!(count.content, "1 match in 1 file\n\n./\n  deep.txt:1");
}

#[tokio::test]
async fn multiline_matches_span_lines_only_when_enabled() {
    let dir = fixture();
    std::fs::write(dir.path().join("span.txt"), "fn open(\n    a: u8,\n) {}\n").unwrap();
    let single = search(&dir, r"open\(\n\s+a", |_| {}).await;
    assert_eq!(single.content, "No matches found");

    let multi = search(&dir, r"open\(\n\s+a", |args| args.multiline = Some(true)).await;
    assert_eq!(multi.content, "1 match in 1 file\n\nspan.txt\n1:fn open(\n2:    a: u8,");
}

#[tokio::test]
async fn binary_files_are_skipped() {
    let dir = fixture();
    let mut blob = b"\x00binary\x00".to_vec();
    blob.extend_from_slice(b"needle\n");
    std::fs::write(dir.path().join("blob.bin"), blob).unwrap();
    let result = search(&dir, "needle", |_| {}).await;
    assert_eq!(result.content, "No matches found");
}

#[tokio::test]
async fn oversized_files_are_skipped_but_large_ones_are_searched() {
    let dir = fixture();
    let over = format!("{}\nneedle\n", "padding ".repeat(130_000));
    std::fs::write(dir.path().join("over.txt"), over).unwrap();
    let under = format!("{}\nneedle\n", "padding ".repeat(100_000));
    std::fs::write(dir.path().join("under.txt"), under).unwrap();

    let result = search(&dir, "needle", |_| {}).await;
    assert!(result.content.contains("\nunder.txt\n"));
    assert!(!result.content.contains("over.txt"));
}

#[tokio::test]
async fn type_filter_scopes_matches_to_source_files() {
    let dir = fixture();
    let unfiltered = search(&dir, "pub", |_| {}).await;
    assert!(unfiltered.content.starts_with("4 matches in 3 files\n"));

    let filtered = search(&dir, "pub", |args| args.file_type = Some("rust".to_string())).await;
    assert_eq!(
        filtered.content,
        "3 matches in 2 files\n\nsrc/lib.rs\n1:pub mod ui;\n\nsrc/ui/widget.rs\n1:pub struct Widget;\n2:pub enum Kind { A, B }"
    );

    let unknown = search(&dir, "pub", |args| args.file_type = Some("nosuchtype".to_string())).await;
    assert!(unknown.is_error);
    assert!(unknown.content.contains("unknown type"));
}

#[tokio::test]
async fn gitignore_rules_are_respected_by_default() {
    let dir = fixture();
    let result = search(&dir, "secret", |_| {}).await;
    assert_eq!(result.content, "No matches found");
}

#[tokio::test]
async fn hidden_flag_includes_hidden_files_while_respecting_gitignore() {
    let dir = fixture();
    let ignored = search(&dir, "secret", |args| args.hidden = Some(true)).await;
    assert_eq!(ignored.content, "No matches found");

    let dotfile = search(&dir, "hidden todo", |args| args.hidden = Some(true)).await;
    assert_eq!(dotfile.content, "1 match in 1 file\n\n.hidden_file\n1:hidden todo");
}
