//! Shared truncation utilities for tool outputs, ported from pi's
//! `truncate.ts`. Two independent limits apply - whichever is hit first wins:
//! a line limit (default 2000) and a byte limit (default 50KB). Neither
//! function returns partial lines; head truncation reports an oversized first
//! line through `first_line_exceeds_limit` and tail truncation reports a
//! partial final line through `last_line_partial`.

use std::borrow::Cow;
use std::path::Path;

pub const DEFAULT_MAX_LINES: usize = 2000;
pub const DEFAULT_MAX_BYTES: usize = 50 * 1024; // 50 KB
/// Max chars per search match line (pi's `GREP_MAX_LINE_LENGTH`).
pub const GREP_MAX_LINE_LENGTH: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TruncatedBy {
    Lines,
    Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Truncation {
    pub content: String,
    pub truncated: bool,
    pub truncated_by: Option<TruncatedBy>,
    pub total_lines: usize,
    pub total_bytes: usize,
    pub output_lines: usize,
    pub output_bytes: usize,
    /// Tail edge case: the first kept line was partially truncated from the end.
    pub last_line_partial: bool,
    /// Head edge case: the first line alone exceeded the byte limit, so no
    /// content was emitted.
    pub first_line_exceeds_limit: bool,
    pub max_lines: usize,
    pub max_bytes: usize,
}

/// Format bytes as human-readable size (pi's `formatSize`).
pub fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// A single line passed through [`truncate_line`].
pub struct TruncatedLine<'a> {
    pub text: Cow<'a, str>,
    pub was_truncated: bool,
}

/// Truncate a single line to [`GREP_MAX_LINE_LENGTH`] chars, appending
/// `... [truncated]` (pi's `truncateLine`, used for search match lines).
pub fn truncate_line(line: &str) -> TruncatedLine<'_> {
    if line.len() <= GREP_MAX_LINE_LENGTH {
        return TruncatedLine {
            text: Cow::Borrowed(line),
            was_truncated: false,
        };
    }
    match line.char_indices().nth(GREP_MAX_LINE_LENGTH) {
        None => TruncatedLine {
            text: Cow::Borrowed(line),
            was_truncated: false,
        },
        Some((byte_idx, _)) => TruncatedLine {
            text: Cow::Owned(format!("{}... [truncated]", &line[..byte_idx])),
            was_truncated: true,
        },
    }
}

fn untruncated(
    content: &str,
    total_lines: usize,
    total_bytes: usize,
    max_lines: usize,
    max_bytes: usize,
) -> Truncation {
    Truncation {
        content: content.to_string(),
        truncated: false,
        truncated_by: None,
        total_lines,
        total_bytes,
        output_lines: total_lines,
        output_bytes: total_bytes,
        last_line_partial: false,
        first_line_exceeds_limit: false,
        max_lines,
        max_bytes,
    }
}

fn collect_head_lines<'a>(
    lines: impl Iterator<Item = &'a str>,
    max_lines: usize,
    max_bytes: usize,
) -> (Vec<&'a str>, TruncatedBy) {
    let mut kept = Vec::new();
    let mut output_bytes = 0usize;
    let mut truncated_by = TruncatedBy::Lines;
    for (i, line) in lines.enumerate().take(max_lines) {
        let line_bytes = line.len() + usize::from(i > 0);
        if output_bytes + line_bytes > max_bytes {
            return (kept, TruncatedBy::Bytes);
        }
        kept.push(line);
        output_bytes += line_bytes;
    }
    if kept.len() >= max_lines && output_bytes <= max_bytes {
        truncated_by = TruncatedBy::Lines;
    }
    (kept, truncated_by)
}

/// Truncate content from the head (keep the first N lines/bytes). Suitable for
/// file reads where the beginning matters.
pub fn truncate_head(content: &str, max_lines: usize, max_bytes: usize) -> Truncation {
    let (total_bytes, total_lines) = (content.len(), content.lines().count());
    if total_lines <= max_lines && total_bytes <= max_bytes {
        return untruncated(content, total_lines, total_bytes, max_lines, max_bytes);
    }
    if let Some(first_line) = content.lines().next()
        && first_line.len() > max_bytes
    {
        return Truncation {
            content: String::new(),
            truncated: true,
            truncated_by: Some(TruncatedBy::Bytes),
            total_lines,
            total_bytes,
            output_lines: 0,
            output_bytes: 0,
            last_line_partial: false,
            first_line_exceeds_limit: true,
            max_lines,
            max_bytes,
        };
    }
    let (kept, truncated_by) = collect_head_lines(content.lines(), max_lines, max_bytes);
    let output_lines = kept.len();
    let out = kept.join("\n");
    Truncation {
        output_bytes: out.len(),
        output_lines,
        content: out,
        truncated: true,
        truncated_by: Some(truncated_by),
        total_lines,
        total_bytes,
        last_line_partial: false,
        first_line_exceeds_limit: false,
        max_lines,
        max_bytes,
    }
}

/// Truncate a string to fit within a byte limit counted from the end, keeping
/// a valid UTF-8 character boundary.
fn truncate_string_to_bytes_from_end(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let start = s.len().saturating_sub(max_bytes);
    let mut boundary = start;
    while boundary < s.len() && !s.is_char_boundary(boundary) {
        boundary += 1;
    }
    &s[boundary..]
}

fn collect_tail_lines<'a>(
    lines: impl Iterator<Item = &'a str>,
    max_lines: usize,
    max_bytes: usize,
) -> (Vec<&'a str>, TruncatedBy, bool) {
    let mut out_rev = Vec::new();
    let mut bytes_count = 0_usize;
    let mut truncated_by = TruncatedBy::Lines;
    let mut partial = false;

    for line in lines {
        let add = line.len().saturating_add(usize::from(!out_rev.is_empty()));
        if bytes_count.saturating_add(add) > max_bytes {
            truncated_by = TruncatedBy::Bytes;
            if out_rev.is_empty() {
                out_rev.push(truncate_string_to_bytes_from_end(line, max_bytes));
                partial = true;
            }
            break;
        }
        out_rev.push(line);
        bytes_count = bytes_count.saturating_add(add);
        if out_rev.len() >= max_lines {
            break;
        }
    }
    out_rev.reverse();
    (out_rev, truncated_by, partial)
}

/// Truncate content from the tail (keep the last N lines/bytes). Suitable for
/// bash output where the end matters (errors, final results). May return a
/// partial first line when the last line alone exceeds the byte limit.
pub fn truncate_tail(content: &str, max_lines: usize, max_bytes: usize) -> Truncation {
    let (total_bytes, total_lines) = (content.len(), content.lines().count());
    if total_lines <= max_lines && total_bytes <= max_bytes {
        return untruncated(content, total_lines, total_bytes, max_lines, max_bytes);
    }
    let (out, truncated_by, partial) = collect_tail_lines(content.lines().rev(), max_lines, max_bytes);
    let output_lines = out.len();
    let content = out.join("\n");
    Truncation {
        output_bytes: content.len(),
        output_lines,
        content,
        truncated: true,
        truncated_by: Some(truncated_by),
        total_lines,
        total_bytes,
        last_line_partial: partial,
        first_line_exceeds_limit: false,
        max_lines,
        max_bytes,
    }
}

pub fn truncate_head_with_spill(
    content: &str,
    max_lines: usize,
    max_bytes: usize,
    artifact_dir: Option<&Path>,
) -> Truncation {
    let mut trunc = truncate_head(content, max_lines, max_bytes);
    if trunc.truncated
        && let Some(dir) = artifact_dir
        && let Ok(path) = crate::tools::artifact::spill_artifact(dir, content)
    {
        let notice = crate::tools::artifact::format_artifact_notice(&path, trunc.total_lines, trunc.total_bytes);
        trunc.content.push_str("\n\n");
        trunc.content.push_str(&notice);
    }
    trunc
}

pub fn truncate_tail_with_spill(
    content: &str,
    max_lines: usize,
    max_bytes: usize,
    artifact_dir: Option<&Path>,
) -> Truncation {
    let mut trunc = truncate_tail(content, max_lines, max_bytes);
    if trunc.truncated
        && let Some(dir) = artifact_dir
        && let Ok(path) = crate::tools::artifact::spill_artifact(dir, content)
    {
        let notice = crate::tools::artifact::format_artifact_notice(&path, trunc.total_lines, trunc.total_bytes);
        trunc.content.push_str("\n\n");
        trunc.content.push_str(&notice);
    }
    trunc
}

pub fn split_artifact_notice(content: &str) -> (&str, Option<&str>) {
    const SPILL_PREFIX: &str = "\n\n[Output truncated. Full content";
    if let Some(idx) = content.rfind(SPILL_PREFIX) {
        (&content[..idx], Some(&content[idx + 2..]))
    } else {
        (content, None)
    }
}

#[cfg(test)]
mod tests {
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
        let input = "x".repeat(600);
        let res = truncate_line(&input);
        assert_eq!(res.text, format!("{}... [truncated]", "x".repeat(GREP_MAX_LINE_LENGTH)));
        assert!(res.was_truncated);
    }

    #[test]
    fn test_truncate_line_counts_multibyte_chars_individually() {
        let input = "é".repeat(600);
        let res = truncate_line(&input);
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
        let actual = (
            res.truncated,
            res.truncated_by,
            res.first_line_exceeds_limit,
            res.content.as_str(),
            res.output_lines,
        );
        assert_eq!(actual, (true, Some(TruncatedBy::Bytes), true, "", 0));
    }

    #[test]
    fn test_truncate_head_counts_joining_newline() {
        let res = truncate_head("aaaa\nbbbb\ncccc", 10, 8);
        assert!(res.truncated);
        assert_eq!(res.truncated_by, Some(TruncatedBy::Bytes));
        assert_eq!(res.content, "aaaa");
        assert_eq!(res.output_bytes, 4);
    }

    #[test]
    fn test_truncate_head_counts_multibyte_characters_as_bytes() {
        let line = "é".repeat(20_000);
        let text = format!("{line}\n{line}\n{line}");
        let res = truncate_head(&text, 10, 51200);
        let actual = (
            res.truncated,
            res.truncated_by,
            res.first_line_exceeds_limit,
            res.output_lines,
            res.output_bytes,
        );
        assert_eq!(actual, (true, Some(TruncatedBy::Bytes), false, 1, 40_000));
    }

    #[test]
    fn test_truncate_tail_within_limits() {
        let text = "line 1\nline 2\nline 3";
        let res = truncate_tail(text, 10, 100);
        assert!(!res.truncated);
        assert_eq!(res.content, text);
        assert_eq!(res.output_lines, 3);
        assert_eq!(res.output_bytes, text.len());
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
        assert!(!res.last_line_partial);
    }

    #[test]
    fn test_truncate_tail_by_bytes() {
        let text = "aaaa\nbbbb\ncccc\ndddd";
        let res = truncate_tail(text, 10, 9);
        assert!(res.truncated);
        assert_eq!(res.truncated_by, Some(TruncatedBy::Bytes));
        assert_eq!(res.output_bytes, 9);
        assert_eq!(res.content, "cccc\ndddd");
        assert!(!res.last_line_partial);
    }

    #[test]
    fn test_truncate_tail_single_oversized_line() {
        let text = "1234567890abcdef";
        let res = truncate_tail(text, 10, 6);
        assert!(res.truncated);
        assert_eq!(res.truncated_by, Some(TruncatedBy::Bytes));
        assert_eq!(res.content, "abcdef");
        assert!(res.last_line_partial);
    }

    #[test]
    fn test_truncate_head_with_spill_writes_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let content = "line 1\nline 2\nline 3\nline 4\nline 5";
        let res = truncate_head_with_spill(content, 2, 1000, Some(dir.path()));

        assert!(res.truncated);
        assert!(res.content.contains("line 1\nline 2"));
        assert!(res.content.contains("Full content (5 lines, 34B) saved to"));

        let files: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(files.len(), 1);
        let saved_content = std::fs::read_to_string(files[0].as_ref().unwrap().path()).unwrap();
        assert_eq!(saved_content, content);
    }

    #[test]
    fn test_truncate_tail_with_spill_writes_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let content = "line 1\nline 2\nline 3\nline 4\nline 5";
        let res = truncate_tail_with_spill(content, 2, 1000, Some(dir.path()));

        assert!(res.truncated);
        assert!(res.content.contains("line 4\nline 5"));
        assert!(res.content.contains("Full content (5 lines, 34B) saved to"));
    }

    #[test]
    fn test_split_artifact_notice() {
        let text = "main content\n\n[Output truncated. Full content (10 lines, 100B) saved to /tmp/foo.log]";
        let (body, notice) = split_artifact_notice(text);
        assert_eq!(body, "main content");
        assert_eq!(
            notice,
            Some("[Output truncated. Full content (10 lines, 100B) saved to /tmp/foo.log]")
        );

        let clean = "clean content without notice";
        let (body2, notice2) = split_artifact_notice(clean);
        assert_eq!(body2, clean);
        assert_eq!(notice2, None);
    }
}
