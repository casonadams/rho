use super::{TruncatedBy, Truncation};

/// Truncate content from the head (keep the first N lines/bytes). Suitable for
/// file reads where the beginning matters.
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

fn first_line_oversized(total_lines: usize, total_bytes: usize, max_lines: usize, max_bytes: usize) -> Truncation {
    Truncation {
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

pub fn truncate_head(content: &str, max_lines: usize, max_bytes: usize) -> Truncation {
    let (total_bytes, total_lines) = (content.len(), content.lines().count());
    if total_lines <= max_lines && total_bytes <= max_bytes {
        return untruncated(content, total_lines, total_bytes, max_lines, max_bytes);
    }
    if let Some(first_line) = content.lines().next()
        && first_line.len() > max_bytes
    {
        return first_line_oversized(total_lines, total_bytes, max_lines, max_bytes);
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
