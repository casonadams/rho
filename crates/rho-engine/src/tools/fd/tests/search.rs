use super::{find, fixture};
use crate::tools::fd::{FdArgs, FdTool};

#[tokio::test]
async fn matches_unanchored_against_workspace_relative_paths() {
    let dir = fixture();
    let result = find(&dir, "widget", |_| {}).await;
    assert!(!result.is_error);
    assert_eq!(result.content, "1 path\n\nsrc/ui/\n  widget.rs");

    let alternation = find(&dir, "main|lib", |_| {}).await;
    assert_eq!(alternation.content, "2 paths\n\nsrc/\n  lib.rs\n  main.rs");
}

#[tokio::test]
async fn pattern_is_smart_case() {
    let dir = fixture();
    let upper = find(&dir, "WIDGET", |_| {}).await;
    assert_eq!(upper.content, "No paths found");

    let lower = find(&dir, "widget", |_| {}).await;
    assert_eq!(lower.content, "1 path\n\nsrc/ui/\n  widget.rs");

    let mixed = find(&dir, "Widget", |_| {}).await;
    assert_eq!(mixed.content, "No paths found");
}

#[tokio::test]
async fn output_is_sorted_before_truncation() {
    let dir = fixture();
    let result = find(&dir, "rs$|md$", |_| {}).await;
    assert_eq!(
        result.content,
        "4 paths\n\n./\n  README.md\nsrc/\n  lib.rs\n  main.rs\nsrc/ui/\n  widget.rs"
    );
}

#[tokio::test]
async fn type_filter_keeps_only_matching_extensions() {
    let dir = fixture();
    let result = find(&dir, ".", |args| args.file_type = Some("rust".to_string())).await;
    assert_eq!(
        result.content,
        "3 paths\n\nsrc/\n  lib.rs\n  main.rs\nsrc/ui/\n  widget.rs"
    );

    let blank = find(&dir, ".", |args| args.file_type = Some("   ".to_string())).await;
    assert_eq!(
        blank.content,
        "4 paths\n\n./\n  README.md\nsrc/\n  lib.rs\n  main.rs\nsrc/ui/\n  widget.rs"
    );

    let unknown = find(&dir, ".", |args| args.file_type = Some("nosuchtype".to_string())).await;
    assert!(unknown.is_error);
    assert!(unknown.content.contains("unknown type"));
}

#[tokio::test]
async fn depth_bounds_traversal() {
    let dir = fixture();
    let one = find(&dir, ".", |args| args.depth = Some(1)).await;
    assert_eq!(one.content, "2 paths\n\n./\n  README.md\n  src/");

    let two = find(&dir, ".", |args| args.depth = Some(2)).await;
    assert_eq!(
        two.content,
        "4 paths\n\n./\n  README.md\nsrc/\n  lib.rs\n  main.rs\n  ui/"
    );
}

#[tokio::test]
async fn gitignore_rules_are_respected_by_default() {
    let dir = fixture();
    let result = find(&dir, "notes", |_| {}).await;
    assert!(!result.is_error);
    assert_eq!(result.content, "No paths found");
}

#[tokio::test]
async fn hidden_flag_includes_hidden_files_while_respecting_gitignore() {
    let dir = fixture();
    let ignored = find(&dir, "notes", |args| args.hidden = Some(true)).await;
    assert_eq!(ignored.content, "No paths found");

    let dotfile = find(&dir, "hidden", |args| args.hidden = Some(true)).await;
    assert_eq!(dotfile.content, "1 path\n\n./\n  .hidden_file");
}

#[tokio::test]
async fn quoted_boolean_and_integer_arguments_are_accepted() {
    use crate::tools::engine_tool::EngineTool;
    let dir = fixture();
    let args = serde_json::json!({ "pattern": "hidden", "hidden": "true", "limit": "5" });
    let result = EngineTool::execute(&FdTool::new(dir.path()), args).await.unwrap();
    assert!(!result.is_error, "{}", result.content);
    assert_eq!(result.content, "1 path\n\n./\n  .hidden_file");
}

#[tokio::test]
async fn no_pattern_or_empty_pattern_matches_all_files() {
    let dir = fixture();
    let no_pat = FdTool::new(dir.path()).execute(FdArgs::default()).await.unwrap();
    assert!(!no_pat.is_error);
    assert!(no_pat.content.contains("src/ui/\n  widget.rs"));
    assert!(no_pat.content.contains("  README.md"));

    let empty = find(&dir, "   ", |_| {}).await;
    assert!(!empty.is_error);
    assert_eq!(empty.content, no_pat.content);
}
