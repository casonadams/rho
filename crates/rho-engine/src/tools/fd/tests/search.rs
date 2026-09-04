use super::{find, fixture};
use crate::tools::fd::{FdArgs, FdTool};

#[tokio::test]
async fn matches_unanchored_against_workspace_relative_paths() {
    let dir = fixture();
    let result = find(&dir, "widget", |_| {}).await;
    assert!(!result.is_error);
    assert_eq!(result.content, "src/ui/widget.rs");

    let alternation = find(&dir, "main|lib", |_| {}).await;
    assert_eq!(
        alternation.content.lines().collect::<Vec<_>>(),
        ["src/lib.rs", "src/main.rs"]
    );
}

#[tokio::test]
async fn pattern_is_smart_case() {
    let dir = fixture();
    let upper = find(&dir, "WIDGET", |_| {}).await;
    assert_eq!(upper.content, "No files found matching pattern");

    let lower = find(&dir, "widget", |_| {}).await;
    assert_eq!(lower.content, "src/ui/widget.rs");

    let mixed = find(&dir, "Widget", |_| {}).await;
    assert_eq!(mixed.content, "No files found matching pattern");
}

#[tokio::test]
async fn output_is_sorted_before_truncation() {
    let dir = fixture();
    let result = find(&dir, "rs$|md$", |_| {}).await;
    assert_eq!(
        result.content.lines().collect::<Vec<_>>(),
        ["README.md", "src/lib.rs", "src/main.rs", "src/ui/widget.rs"]
    );
}

#[tokio::test]
async fn type_filter_keeps_only_matching_extensions() {
    let dir = fixture();
    let result = find(&dir, ".", |args| args.file_type = Some("rust".to_string())).await;
    assert_eq!(
        result.content.lines().collect::<Vec<_>>(),
        ["src/lib.rs", "src/main.rs", "src/ui/widget.rs"]
    );

    let unknown = find(&dir, ".", |args| args.file_type = Some("nosuchtype".to_string())).await;
    assert!(unknown.is_error);
    assert!(unknown.content.contains("unknown type"));
}

#[tokio::test]
async fn depth_bounds_traversal() {
    let dir = fixture();
    let one = find(&dir, ".", |args| args.depth = Some(1)).await;
    assert_eq!(one.content.lines().collect::<Vec<_>>(), ["README.md", "src"]);

    let two = find(&dir, ".", |args| args.depth = Some(2)).await;
    assert_eq!(
        two.content.lines().collect::<Vec<_>>(),
        ["README.md", "src", "src/lib.rs", "src/main.rs", "src/ui"]
    );
}

#[tokio::test]
async fn gitignore_rules_are_respected_by_default() {
    let dir = fixture();
    let result = find(&dir, "notes", |_| {}).await;
    assert!(!result.is_error);
    assert_eq!(result.content, "No files found matching pattern");
}

#[tokio::test]
async fn hidden_flag_includes_hidden_and_ignored_entries() {
    let dir = fixture();
    let ignored = find(&dir, "notes", |args| args.hidden = Some(true)).await;
    assert_eq!(ignored.content, "notes.txt");

    let dotfile = find(&dir, "hidden", |args| args.hidden = Some(true)).await;
    assert_eq!(dotfile.content, ".hidden_file");
}

#[tokio::test]
async fn no_pattern_or_empty_pattern_matches_all_files() {
    let dir = fixture();
    let no_pat = FdTool::new(dir.path()).execute(FdArgs::default()).await.unwrap();
    assert!(!no_pat.is_error);
    assert!(no_pat.content.contains("src/ui/widget.rs"));
    assert!(no_pat.content.contains("README.md"));

    let empty = find(&dir, "   ", |_| {}).await;
    assert!(!empty.is_error);
    assert_eq!(empty.content, no_pat.content);
}
