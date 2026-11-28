use super::{find, fixture};
use crate::tools::fd::FdSort;
use crate::tools::fd::stats::count_file_stats;
use rho_test_support::TestDir;
use test_case::test_case;

fn table(content: &str) -> Vec<&str> {
    content.lines().skip(2).collect()
}

fn write_numbered_lines(path: &std::path::Path, count: usize) {
    let content = (0..count)
        .map(|i| format!("// line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(path, content).unwrap();
}

#[tokio::test]
async fn stats_flag_renders_table_with_lines_and_bytes() {
    let dir = fixture();
    let result = find(&dir, "widget", |args| args.stats = Some(true)).await;
    assert!(!result.is_error);
    assert!(result.content.starts_with("1 path\n\n"));
    let lines = table(&result.content);
    assert_eq!(lines[0], "  Lines    Bytes  Path");
    assert!(lines[1].contains("src/ui/widget.rs"));
    assert!(lines[1].contains("1"));
}

#[tokio::test]
async fn min_lines_filters_out_smaller_files_and_directories() {
    let dir = fixture();
    write_numbered_lines(&dir.join("src/big.rs"), 200);

    let result = find(&dir, ".*\\.rs$", |args| args.min_lines = Some(150)).await;
    assert!(!result.is_error);
    let lines = table(&result.content);
    assert_eq!(lines[0].trim(), "Lines    Bytes  Path");
    assert_eq!(lines.len(), 2);
    assert!(lines[1].contains("src/big.rs"));
    assert!(lines[1].contains("200"));
}

#[tokio::test]
async fn max_lines_filters_out_larger_files() {
    let dir = fixture();
    write_numbered_lines(&dir.join("src/big.rs"), 200);

    let result = find(&dir, ".*\\.rs$", |args| args.max_lines = Some(10)).await;
    assert!(!result.is_error);
    assert!(!result.content.contains("src/big.rs"));
    assert!(result.content.contains("src/main.rs"));
    assert!(result.content.contains("src/lib.rs"));
}

#[tokio::test]
async fn sort_by_lines_orders_descending() {
    let dir = fixture();
    write_numbered_lines(&dir.join("src/medium.rs"), 50);
    write_numbered_lines(&dir.join("src/big.rs"), 100);

    let result = find(&dir, ".*\\.rs$", |args| args.sort = Some(FdSort::Lines)).await;
    assert!(!result.is_error);
    let lines = table(&result.content);
    // Header is row 0
    assert!(lines[1].contains("src/big.rs"));
    assert!(lines[2].contains("src/medium.rs"));
}

#[tokio::test]
async fn sort_by_size_orders_descending() {
    let dir = fixture();
    let huge_comment = dir.join("src/huge.rs");
    // Single line but large byte size
    std::fs::write(huge_comment, "// ".to_string() + &"x".repeat(5000) + "\n").unwrap();

    let result = find(&dir, ".*\\.rs$", |args| args.sort = Some(FdSort::Size)).await;
    assert!(!result.is_error);
    let lines = table(&result.content);
    assert!(lines[1].contains("src/huge.rs"));
}

#[tokio::test]
async fn sort_by_size_with_stats_reports_line_and_byte_counts() {
    let dir = fixture();
    std::fs::write(dir.join("src/many_lines.rs"), "a\n".repeat(50)).unwrap();
    std::fs::write(dir.join("src/one_line.rs"), "x".repeat(400)).unwrap();

    let result = find(&dir, ".*\\.rs$", |args| {
        args.sort = Some(FdSort::Size);
        args.stats = Some(true);
    })
    .await;
    assert!(!result.is_error);
    let rows = table(&result.content);
    assert!(rows[1].contains("src/one_line.rs"));
    assert!(rows[1].trim_start().starts_with("1 "));
    assert!(rows[2].contains("src/many_lines.rs"));
    assert!(rows[2].trim_start().starts_with("50 "));
}

#[test_case("", 0, 0; "empty")]
#[test_case("hello", 1, 5; "one line without newline")]
#[test_case("hello\n", 1, 6; "one line with newline")]
#[test_case("hello\nworld", 2, 11; "two lines without newline")]
fn test_count_file_stats_semantics(content: &str, lines: usize, bytes: u64) {
    let temp = TestDir::new();
    let path = temp.join("file.txt");
    std::fs::write(&path, content).unwrap();
    let stats = count_file_stats(&path).unwrap();
    assert_eq!((stats.lines, stats.bytes), (lines, bytes));
}
