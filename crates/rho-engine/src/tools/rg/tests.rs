use super::*;
use tempfile::TempDir;

fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("src/ui")).unwrap();
    std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "pub mod ui;\n").unwrap();
    std::fs::write(
        dir.path().join("src/ui/widget.rs"),
        "pub struct Widget;\npub enum Kind { A, B }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("README.md"), "# Fixture\ntodo list\npub markdown\n").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "secret notes\n").unwrap();
    std::fs::write(dir.path().join(".hidden_file"), "hidden todo\n").unwrap();
    std::fs::write(dir.path().join(".gitignore"), "notes.txt\n").unwrap();
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    dir
}

async fn search(dir: &TempDir, pattern: &str, mutate: impl FnOnce(&mut RgArgs)) -> ToolResult {
    let mut args = RgArgs {
        pattern: pattern.to_string(),
        path: None,
        file_type: None,
        hidden: None,
        limit: None,
    };
    mutate(&mut args);
    RgTool::new(dir.path()).execute(args).await.unwrap()
}

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
    assert_eq!(upper.content, "No matches.");

    let mixed = search(&dir, "Todo", |_| {}).await;
    assert_eq!(mixed.content, "No matches.");
}

#[tokio::test]
async fn binary_files_are_skipped() {
    let dir = fixture();
    let mut blob = b"\x00binary\x00".to_vec();
    blob.extend_from_slice(b"needle\n");
    std::fs::write(dir.path().join("blob.bin"), blob).unwrap();
    let result = search(&dir, "needle", |_| {}).await;
    assert_eq!(result.content, "No matches.");
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
async fn long_match_lines_are_truncated_with_an_ellipsis() {
    let dir = fixture();
    std::fs::write(dir.path().join("long.txt"), format!("needle{}\n", "x".repeat(600))).unwrap();
    let result = search(&dir, "needle", |_| {}).await;
    assert!(!result.is_error);
    let text = result.content.strip_prefix("long.txt:1: ").unwrap();
    assert_eq!(text.chars().count(), 501);
    assert!(text.ends_with('\u{2026}'));
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
    assert_eq!(result.content, "No matches.");
}

#[tokio::test]
async fn hidden_flag_includes_hidden_and_ignored_entries() {
    let dir = fixture();
    let ignored = search(&dir, "secret", |args| args.hidden = Some(true)).await;
    assert_eq!(ignored.content, "notes.txt:1: secret notes");

    let dotfile = search(&dir, "hidden todo", |args| args.hidden = Some(true)).await;
    assert_eq!(dotfile.content, ".hidden_file:1: hidden todo");
}

#[tokio::test]
async fn limit_truncates_with_a_narrowing_notice() {
    let dir = fixture();
    let result = search(&dir, ".", |args| args.limit = Some(2)).await;
    assert!(!result.is_error);
    assert_eq!(
        result.content,
        "README.md:1: # Fixture\nREADME.md:2: todo list\n[showing first 2 of 7 matches; narrow with a tighter pattern, path, or type]"
    );
}

#[tokio::test]
async fn limit_clamps_to_at_least_one() {
    let dir = fixture();
    let result = search(&dir, ".", |args| args.limit = Some(0)).await;
    assert_eq!(
        result.content,
        "README.md:1: # Fixture\n[showing first 1 of 7 matches; narrow with a tighter pattern, path, or type]"
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
    assert_eq!(lines.len(), 201);
    assert_eq!(lines[0], "many.txt:1: needle line 0");
    assert_eq!(
        lines[200],
        "[showing first 200 of 5000+ matches (collection ceiling reached); narrow with a tighter pattern, path, or type]"
    );
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
