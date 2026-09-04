use super::entry::format_results;
use super::stats::count_file_stats;
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
        pattern: Some(pattern.to_string()),
        ..Default::default()
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
    assert!(schema["properties"].get("stats").is_some());
    assert!(schema["properties"].get("min_lines").is_some());
    assert!(schema["properties"].get("max_lines").is_some());
    assert!(schema["properties"].get("sort").is_some());
    let required = schema.get("required").and_then(|r| r.as_array());
    assert!(
        required.is_none() || !required.unwrap().iter().any(|v| v == "pattern"),
        "pattern should be optional in schema"
    );
}

#[tokio::test]
async fn stats_flag_renders_table_with_lines_and_bytes() {
    let dir = fixture();
    let result = find(&dir, "widget", |args| args.stats = Some(true)).await;
    assert!(!result.is_error);
    let lines: Vec<_> = result.content.lines().collect();
    assert_eq!(lines[0].trim(), "Lines    Bytes  Path");
    assert!(lines[1].contains("src/ui/widget.rs"));
    assert!(lines[1].contains("1"));
}

#[tokio::test]
async fn min_lines_filters_out_smaller_files_and_directories() {
    let dir = fixture();
    let big_path = dir.path().join("src/big.rs");
    let big_content = (0..200).map(|i| format!("// line {i}")).collect::<Vec<_>>().join("\n");
    std::fs::write(big_path, big_content).unwrap();

    let result = find(&dir, ".*\\.rs$", |args| args.min_lines = Some(150)).await;
    assert!(!result.is_error);
    let lines: Vec<_> = result.content.lines().collect();
    assert_eq!(lines[0].trim(), "Lines    Bytes  Path");
    assert_eq!(lines.len(), 2);
    assert!(lines[1].contains("src/big.rs"));
    assert!(lines[1].contains("200"));
}

#[tokio::test]
async fn max_lines_filters_out_larger_files() {
    let dir = fixture();
    let big_path = dir.path().join("src/big.rs");
    let big_content = (0..200).map(|i| format!("// line {i}")).collect::<Vec<_>>().join("\n");
    std::fs::write(big_path, big_content).unwrap();

    let result = find(&dir, ".*\\.rs$", |args| args.max_lines = Some(10)).await;
    assert!(!result.is_error);
    assert!(!result.content.contains("src/big.rs"));
    assert!(result.content.contains("src/main.rs"));
    assert!(result.content.contains("src/lib.rs"));
}

#[tokio::test]
async fn sort_by_lines_orders_descending() {
    let dir = fixture();
    let medium_path = dir.path().join("src/medium.rs");
    let medium_content = (0..50).map(|i| format!("// line {i}")).collect::<Vec<_>>().join("\n");
    std::fs::write(medium_path, medium_content).unwrap();

    let big_path = dir.path().join("src/big.rs");
    let big_content = (0..100).map(|i| format!("// line {i}")).collect::<Vec<_>>().join("\n");
    std::fs::write(big_path, big_content).unwrap();

    let result = find(&dir, ".*\\.rs$", |args| args.sort = Some(FdSort::Lines)).await;
    assert!(!result.is_error);
    let lines: Vec<_> = result.content.lines().collect();
    // Header is row 0
    assert!(lines[1].contains("src/big.rs"));
    assert!(lines[2].contains("src/medium.rs"));
}

#[tokio::test]
async fn sort_by_size_orders_descending() {
    let dir = fixture();
    let huge_comment = dir.path().join("src/huge.rs");
    // Single line but large byte size
    std::fs::write(huge_comment, "// ".to_string() + &"x".repeat(5000) + "\n").unwrap();

    let result = find(&dir, ".*\\.rs$", |args| args.sort = Some(FdSort::Size)).await;
    assert!(!result.is_error);
    let lines: Vec<_> = result.content.lines().collect();
    assert!(lines[1].contains("src/huge.rs"));
}

#[test]
fn test_count_file_stats_semantics() {
    let temp = TempDir::new().unwrap();

    let empty = temp.path().join("empty.txt");
    std::fs::write(&empty, "").unwrap();
    let stats = count_file_stats(&empty).unwrap();
    assert_eq!(stats.lines, 0);
    assert_eq!(stats.bytes, 0);

    let one_no_nl = temp.path().join("one_no_nl.txt");
    std::fs::write(&one_no_nl, "hello").unwrap();
    let stats = count_file_stats(&one_no_nl).unwrap();
    assert_eq!(stats.lines, 1);
    assert_eq!(stats.bytes, 5);

    let one_with_nl = temp.path().join("one_with_nl.txt");
    std::fs::write(&one_with_nl, "hello\n").unwrap();
    let stats = count_file_stats(&one_with_nl).unwrap();
    assert_eq!(stats.lines, 1);
    assert_eq!(stats.bytes, 6);

    let two_no_nl = temp.path().join("two_no_nl.txt");
    std::fs::write(&two_no_nl, "hello\nworld").unwrap();
    let stats = count_file_stats(&two_no_nl).unwrap();
    assert_eq!(stats.lines, 2);
    assert_eq!(stats.bytes, 11);
}
