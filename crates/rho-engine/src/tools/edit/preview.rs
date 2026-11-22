use crate::tools::truncate::truncate_line;
use similar::{DiffOp, TextDiff};
use std::fmt::Write;

pub const PREVIEW_CONTEXT_LINES: usize = 1;
pub const PREVIEW_MAX_LINES: usize = 60;

/// The changed regions of `after`, numbered like `read` output, capped at `PREVIEW_MAX_LINES` lines.
/// A lone region past the cap keeps its first lines; a pure deletion shows `@@ N` and the count removed.
pub fn changed_regions(before: &str, after: &str) -> String {
    let diff = TextDiff::from_lines(before, after);
    let new_lines: Vec<&str> = after.lines().collect();
    let mut preview = Preview::default();
    for group in diff.grouped_ops(PREVIEW_CONTEXT_LINES) {
        let (start, end) = new_range(&group);
        let end = end.min(new_lines.len());
        if start < end {
            preview.region(&new_lines, start, end);
        } else {
            preview.deletion(start, removed_lines(&group));
        }
    }
    preview.finish()
}

#[derive(Default)]
struct Preview {
    out: String,
    shown: usize,
    omitted: usize,
}

impl Preview {
    fn deletion(&mut self, start: usize, removed: usize) {
        if removed > 0 {
            let plural = if removed == 1 { "" } else { "s" };
            let _ = writeln!(self.out, "@@ {}\n[{removed} line{plural} removed]", start + 1);
            self.shown += 1;
        }
    }

    fn region(&mut self, new_lines: &[&str], start: usize, end: usize) {
        let len = end - start;
        let take = if self.shown == 0 {
            len.min(PREVIEW_MAX_LINES)
        } else if self.shown + len <= PREVIEW_MAX_LINES {
            len
        } else {
            self.omitted += len;
            return;
        };
        let _ = writeln!(self.out, "@@ {}-{}", start + 1, start + take);
        for (index, line) in new_lines[start..start + take].iter().enumerate() {
            let _ = writeln!(self.out, "{}\t{}", start + index + 1, truncate_line(line).text);
        }
        if take < len {
            let _ = writeln!(
                self.out,
                "[{} more lines in this region; read the file to see them]",
                len - take
            );
        }
        self.shown += take;
    }

    fn finish(mut self) -> String {
        if self.omitted > 0 {
            let _ = writeln!(
                self.out,
                "[{} more lines in other changed regions; read the file to see them]",
                self.omitted
            );
        }
        self.out.truncate(self.out.trim_end_matches('\n').len());
        self.out
    }
}

fn removed_lines(group: &[DiffOp]) -> usize {
    group
        .iter()
        .filter(|op| matches!(op, DiffOp::Delete { .. }))
        .map(|op| op.old_range().len())
        .sum()
}

fn new_range(group: &[DiffOp]) -> (usize, usize) {
    let start = group.first().map_or(0, |op| op.new_range().start);
    let end = group.last().map_or(0, |op| op.new_range().end);
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered(count: usize, prefix: &str) -> String {
        (1..=count).map(|i| format!("{prefix}{i}\n")).collect()
    }

    #[test]
    fn a_single_region_past_the_cap_keeps_its_first_lines_with_a_note() {
        let preview = changed_regions("", &numbered(PREVIEW_MAX_LINES + 15, "new "));
        let lines: Vec<&str> = preview.lines().collect();
        assert_eq!(lines[0], format!("@@ 1-{PREVIEW_MAX_LINES}"));
        assert_eq!(lines[1], "1\tnew 1");
        assert_eq!(
            lines[PREVIEW_MAX_LINES],
            format!("{PREVIEW_MAX_LINES}\tnew {PREVIEW_MAX_LINES}")
        );
        assert_eq!(
            lines[PREVIEW_MAX_LINES + 1],
            "[15 more lines in this region; read the file to see them]"
        );
        assert_eq!(lines.len(), PREVIEW_MAX_LINES + 2);
    }

    #[test]
    fn a_pure_deletion_shows_where_and_how_many_lines_were_removed() {
        assert_eq!(changed_regions("a\nb\nc\nd\ne\nf\ng\n", ""), "@@ 1\n[7 lines removed]");
        assert_eq!(changed_regions("a\n", ""), "@@ 1\n[1 line removed]");
    }

    #[test]
    fn a_deletion_with_surrounding_lines_still_lists_them() {
        let preview = changed_regions("a\nb\nc\n", "a\nc\n");
        assert_eq!(preview, "@@ 1-2\n1\ta\n2\tc");
    }
}
