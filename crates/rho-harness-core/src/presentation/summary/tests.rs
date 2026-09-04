use super::*;

#[test]
fn summarize_tool_output_short_ascii() {
    assert_eq!(summarize_tool_output("hello world"), "hello world");
}

#[test]
fn summarize_tool_output_truncates_ascii_over_60() {
    let input = "a".repeat(65);
    assert_eq!(summarize_tool_output(&input), format!("{}...", "a".repeat(60)));
}

#[test]
fn summarize_tool_output_multibyte_boundary_does_not_panic() {
    // 59 ASCII bytes followed by 4-byte unicode characters (e.g. emoji or multi-byte UTF-8).
    // Byte index 60 falls inside the first emoji.
    let input = format!("{}🦀🦀🦀", "a".repeat(59));
    let summary = summarize_tool_output(&input);
    assert_eq!(summary, format!("{}🦀...", "a".repeat(59)));
}

#[test]
fn summarize_tool_output_multibyte_cjk_over_60_chars() {
    let input = "日本語".repeat(30); // 90 CJK characters (270 bytes)
    let summary = summarize_tool_output(&input);
    let expected: String = input.chars().take(60).collect();
    assert_eq!(summary, format!("{expected}..."));
}

#[test]
fn summarize_tool_output_empty_and_multiline() {
    assert_eq!(summarize_tool_output(""), "0 lines");
    assert_eq!(summarize_tool_output("\n\n"), "2 lines");
    assert_eq!(summarize_tool_output("first line\nsecond line"), "first line");
}

#[test]
fn format_tool_args_summary_bash_multibyte_truncation() {
    let cmd = format!("echo {}", "🦀".repeat(70));
    let args = serde_json::json!({ "command": cmd });
    let summary = format_tool_args_summary("bash", &args);
    assert!(summary.starts_with("echo "));
    assert!(summary.ends_with("..."));
    assert!(!summary.contains('`'));
}

#[test]
fn format_tool_args_summary_fd() {
    let with_pattern = serde_json::json!({ "pattern": "widget", "path": "src" });
    assert_eq!(format_tool_args_summary("fd", &with_pattern), "widget in src");

    let without_pattern = serde_json::json!({ "path": "src" });
    assert_eq!(format_tool_args_summary("fd", &without_pattern), "src");

    let default_root = serde_json::json!({});
    assert_eq!(format_tool_args_summary("fd", &default_root), ".");
}
