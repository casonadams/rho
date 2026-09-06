use super::{TruncatedBy, Truncation};

/// Truncate content from the tail (keep the last N lines/bytes). Suitable for
/// bash output where the end matters (errors, final results). May return a
/// partial first line when the last line alone exceeds the byte limit.
fn untruncated(
    content: &str,
    (total_lines, total_bytes): (usize, usize),
    (max_lines, max_bytes): (usize, usize),
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

fn build_tail_result(
    (content, output_lines): (String, usize),
    (total_lines, total_bytes): (usize, usize),
    (max_lines, max_bytes, truncated_by, last_line_partial): (usize, usize, TruncatedBy, bool),
) -> Truncation {
    Truncation {
        output_bytes: content.len(),
        output_lines,
        content,
        truncated: true,
        truncated_by: Some(truncated_by),
        total_lines,
        total_bytes,
        last_line_partial,
        first_line_exceeds_limit: false,
        max_lines,
        max_bytes,
    }
}

pub fn truncate_tail(content: &str, max_lines: usize, max_bytes: usize) -> Truncation {
    let (total_bytes, total_lines) = (content.len(), content.lines().count());
    if total_lines <= max_lines && total_bytes <= max_bytes {
        return untruncated(content, (total_lines, total_bytes), (max_lines, max_bytes));
    }
    let (out, truncated_by, partial) = collect_tail_lines(content.lines().rev(), max_lines, max_bytes);
    let output_lines = out.len();
    let content = out.join("\n");
    build_tail_result(
        (content, output_lines),
        (total_lines, total_bytes),
        (max_lines, max_bytes, truncated_by, partial),
    )
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
