use super::*;
use tempfile::TempDir;

fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("src/ui")).unwrap();
    std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "pub mod ui;\n").unwrap();
    std::fs::write(dir.path().join("src/ui/widget.rs"), "pub struct Widget;\n").unwrap();
    std::fs::write(dir.path().join("README.md"), "# Fixture\n").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "secret notes\n").unwrap();
    std::fs::write(dir.path().join(".hidden_file"), "x\n").unwrap();
    std::fs::write(dir.path().join(".gitignore"), "notes.txt\n").unwrap();
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    dir
}

async fn find(dir: &TempDir, pattern: &str, mutate: impl FnOnce(&mut FdArgs)) -> ToolResult {
    let mut args = FdArgs {
        pattern: pattern.to_string(),
        path: None,
        file_type: None,
        hidden: None,
        depth: None,
        limit: None,
    };
    mutate(&mut args);
    FdTool::new(dir.path()).execute(args).await.unwrap()
}

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
    let result = format_results(paths, false, 250);
    let body = result.content.split("\n\n").next().unwrap();
    assert!(body.len() <= DEFAULT_MAX_BYTES);
    assert!(result.content.contains("showing first 250 of 300 matches"));
    // pi's assembly: notices joined with ". " in one trailing bracket.
    assert!(result.content.ends_with("path, or type. 50.0KB limit reached]"));
}

#[tokio::test]
async fn empty_pattern_is_a_tool_error() {
    let dir = fixture();
    let result = find(&dir, "   ", |_| {}).await;
    assert!(result.is_error);
    assert!(result.content.contains("Empty pattern"));
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
async fn path_outside_the_workspace_errors() {
    let dir = fixture();
    let result = find(&dir, ".", |args| args.path = Some("../elsewhere".to_string())).await;
    assert!(result.is_error);
    assert!(result.content.contains("outside the workspace"));
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
    assert!(schema["properties"].get("type").is_some());
    assert!(schema["properties"].get("file_type").is_none());
    assert!(schema["properties"].get("pattern").is_some());
}
