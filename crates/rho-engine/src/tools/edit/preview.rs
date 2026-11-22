use crate::tools::truncate::truncate_line;
use similar::{DiffOp, TextDiff};
use std::fmt::Write;

pub const PREVIEW_CONTEXT_LINES: usize = 1;
pub const PREVIEW_MAX_LINES: usize = 60;

/// The changed regions of `after`, numbered like `read` output, capped at `PREVIEW_MAX_LINES` lines.
pub fn changed_regions(before: &str, after: &str) -> String {
    let diff = TextDiff::from_lines(before, after);
    let new_lines: Vec<&str> = after.lines().collect();
    let mut out = String::new();
    let mut shown = 0;
    let mut omitted = 0;
    for group in diff.grouped_ops(PREVIEW_CONTEXT_LINES) {
        let (start, end) = new_range(&group);
        let end = end.min(new_lines.len());
        if start >= end {
            continue;
        }
        if shown + (end - start) > PREVIEW_MAX_LINES {
            omitted += end - start;
            continue;
        }
        let _ = writeln!(out, "@@ {}-{}", start + 1, end);
        for (index, line) in new_lines[start..end].iter().enumerate() {
            let _ = writeln!(out, "{}\t{}", start + index + 1, truncate_line(line).text);
        }
        shown += end - start;
    }
    if omitted > 0 {
        let _ = writeln!(
            out,
            "[{omitted} more lines in other changed regions; read the file to see them]"
        );
    }
    out.truncate(out.trim_end_matches('\n').len());
    out
}

fn new_range(group: &[DiffOp]) -> (usize, usize) {
    let start = group.first().map_or(0, |op| op.new_range().start);
    let end = group.last().map_or(0, |op| op.new_range().end);
    (start, end)
}
