use super::{fixture, search};

#[tokio::test]
async fn matches_report_path_line_and_text() {
    let dir = fixture();
    let result = search(&dir, "widget", |_| {}).await;
    assert!(!result.is_error);
    assert_eq!(result.content, "src/ui/widget.rs:1: pub struct Widget;");
}

#[tokio::test]
async fn results_are_ordered_by_path_then_line() {
    let dir = fixture();
    let result = search(&dir, "pub", |_| {}).await;
    assert_eq!(
        result.content.lines().collect::<Vec<_>>(),
        [
            "README.md:3: pub markdown",
            "src/lib.rs:1: pub mod ui;",
            "src/ui/widget.rs:1: pub struct Widget;",
            "src/ui/widget.rs:2: pub enum Kind { A, B }",
        ]
    );
}

#[tokio::test]
async fn pattern_is_smart_case() {
    let dir = fixture();
    let lower = search(&dir, "todo", |_| {}).await;
    assert_eq!(lower.content, "README.md:2: todo list");

    let upper = search(&dir, "TODO", |_| {}).await;
    assert_eq!(upper.content, "No matches found");

    let mixed = search(&dir, "Todo", |_| {}).await;
    assert_eq!(mixed.content, "No matches found");
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
    assert!(result.content.contains("under.txt:"));
    assert!(!result.content.contains("over.txt"));
}

#[tokio::test]
async fn type_filter_scopes_matches_to_source_files() {
    let dir = fixture();
    let unfiltered = search(&dir, "pub", |_| {}).await;
    assert_eq!(unfiltered.content.lines().count(), 4);

    let filtered = search(&dir, "pub", |args| args.file_type = Some("rust".to_string())).await;
    assert_eq!(
        filtered.content.lines().collect::<Vec<_>>(),
        [
            "src/lib.rs:1: pub mod ui;",
            "src/ui/widget.rs:1: pub struct Widget;",
            "src/ui/widget.rs:2: pub enum Kind { A, B }",
        ]
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
async fn hidden_flag_includes_hidden_and_ignored_entries() {
    let dir = fixture();
    let ignored = search(&dir, "secret", |args| args.hidden = Some(true)).await;
    assert_eq!(ignored.content, "notes.txt:1: secret notes");

    let dotfile = search(&dir, "hidden todo", |args| args.hidden = Some(true)).await;
    assert_eq!(dotfile.content, ".hidden_file:1: hidden todo");
}
