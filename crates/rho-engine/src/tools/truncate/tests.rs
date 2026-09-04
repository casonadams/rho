use super::*;

#[test]
fn test_format_size() {
    assert_eq!(format_size(512), "512B");
    assert_eq!(format_size(DEFAULT_MAX_BYTES), "50.0KB");
    assert_eq!(format_size(1024 * 1024), "1.0MB");
}

#[test]
fn test_truncate_line_keeps_short_lines_unmarked() {
    let res = truncate_line("short match");
    assert_eq!(res.text, "short match");
    assert!(!res.was_truncated);
}

#[test]
fn test_truncate_line_at_the_limit_is_unmarked() {
    let line = "a".repeat(GREP_MAX_LINE_LENGTH);
    let res = truncate_line(&line);
    assert_eq!(res.text, line);
    assert!(!res.was_truncated);
}

#[test]
fn test_truncate_line_caps_with_a_marked_suffix() {
    let res = truncate_line(&"x".repeat(600));
    assert_eq!(res.text, format!("{}... [truncated]", "x".repeat(GREP_MAX_LINE_LENGTH)));
    assert!(res.was_truncated);
}

#[test]
fn test_truncate_line_counts_multibyte_chars_individually() {
    let res = truncate_line(&"é".repeat(600));
    assert!(res.was_truncated);
    assert_eq!(
        res.text.chars().count(),
        GREP_MAX_LINE_LENGTH + "... [truncated]".chars().count()
    );
    assert!(res.text.starts_with(&"é".repeat(GREP_MAX_LINE_LENGTH)));
}

#[test]
fn test_truncate_head_within_limits() {
    let text = "line 1\nline 2\nline 3";
    let res = truncate_head(text, 10, 100);
    assert!(!res.truncated);
    assert_eq!(res.content, text);
    assert_eq!(res.output_lines, 3);
    assert_eq!(res.output_bytes, text.len());
}

#[test]
fn test_truncate_head_by_lines() {
    let lines: Vec<String> = (1..=10).map(|i| format!("line {i}")).collect();
    let text = lines.join("\n");
    let res = truncate_head(&text, 3, 1000);
    assert!(res.truncated);
    assert_eq!(res.truncated_by, Some(TruncatedBy::Lines));
    assert_eq!(res.output_lines, 3);
    assert_eq!(res.content, "line 1\nline 2\nline 3");
}

#[test]
fn test_truncate_head_by_bytes() {
    let text = "aaaa\nbbbb\ncccc\ndddd";
    let res = truncate_head(text, 10, 9);
    assert!(res.truncated);
    assert_eq!(res.truncated_by, Some(TruncatedBy::Bytes));
    assert_eq!(res.output_bytes, 9);
    assert_eq!(res.content, "aaaa\nbbbb");
}

#[test]
fn test_truncate_head_first_line_exceeds_limit() {
    let long_line = "abcdefghijklmnopqrstuvwxyz";
    let res = truncate_head(long_line, 10, 5);
    assert!(res.truncated);
    assert_eq!(res.truncated_by, Some(TruncatedBy::Bytes));
    assert!(res.first_line_exceeds_limit);
    assert_eq!(res.content, "");
    assert_eq!(res.output_lines, 0);
}

#[test]
fn test_truncate_head_counts_joining_newline() {
    // 4 + 1 + 4 = 9 bytes joined; a 8-byte limit only fits the first line.
    let res = truncate_head("aaaa\nbbbb\ncccc", 10, 8);
    assert!(res.truncated);
    assert_eq!(res.truncated_by, Some(TruncatedBy::Bytes));
    assert_eq!(res.content, "aaaa");
    assert_eq!(res.output_bytes, 4);
}

#[test]
fn test_truncate_head_counts_multibyte_characters_as_bytes() {
    // Each 'é' is two bytes: 20000 chars = 40000 bytes, so the second line
    // would push the joined output past the byte limit.
    let line = "é".repeat(20_000);
    let text = format!("{line}\n{line}\n{line}");
    let res = truncate_head(&text, 10, 51200);
    assert!(res.truncated);
    assert_eq!(res.truncated_by, Some(TruncatedBy::Bytes));
    assert!(!res.first_line_exceeds_limit);
    assert_eq!(res.output_lines, 1);
    assert_eq!(res.output_bytes, 40_000);
}

#[test]
fn test_truncate_tail_within_limits() {
    let text = "line 1\nline 2\nline 3";
    let res = truncate_tail(text, 10, 100);
    assert!(!res.truncated);
    assert_eq!(res.content, text);
    assert_eq!(res.output_lines, 3);
}

#[test]
fn test_truncate_tail_by_lines() {
    let lines: Vec<String> = (1..=10).map(|i| format!("line {i}")).collect();
    let text = lines.join("\n");
    let res = truncate_tail(&text, 3, 1000);
    assert!(res.truncated);
    assert_eq!(res.truncated_by, Some(TruncatedBy::Lines));
    assert_eq!(res.output_lines, 3);
    assert_eq!(res.content, "line 8\nline 9\nline 10");
}

#[test]
fn test_truncate_tail_by_bytes() {
    let lines = ["aaaa", "bbbb", "cccc", "dddd"];
    let text = lines.join("\n");
    let res = truncate_tail(&text, 10, 9);
    assert!(res.truncated);
    assert_eq!(res.truncated_by, Some(TruncatedBy::Bytes));
    assert_eq!(res.content, "cccc\ndddd");
}

#[test]
fn test_truncate_tail_single_oversized_line() {
    let long_line = "abcdefghijklmnopqrstuvwxyz";
    let res = truncate_tail(long_line, 10, 5);
    assert!(res.truncated);
    assert_eq!(res.truncated_by, Some(TruncatedBy::Bytes));
    assert!(res.last_line_partial);
    assert_eq!(res.content, "vwxyz");
}
