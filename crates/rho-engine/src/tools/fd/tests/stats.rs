use super::{find, fixture};
use crate::tools::fd::FdSort;
use crate::tools::fd::stats::count_file_stats;
use tempfile::TempDir;

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
