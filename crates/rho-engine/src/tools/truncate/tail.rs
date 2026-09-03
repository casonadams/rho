use super::{TruncatedBy, Truncation};

/// Truncate content from the tail (keep the last N lines/bytes). Suitable for
/// bash output where the end matters (errors, final results). May return a
/// partial first line when the last line alone exceeds the byte limit.
pub fn truncate_tail(content: &str, max_lines: usize, max_bytes: usize) -> Truncation {
    let total_bytes = content.len();
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    if total_lines <= max_lines && total_bytes <= max_bytes {
        return Truncation {
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
        };
    }

    let mut output_lines_rev = Vec::new();
    let mut output_bytes_count = 0_usize;
    let mut truncated_by = TruncatedBy::Lines;
    let mut last_line_partial = false;

    for line in lines.iter().rev() {
        let line_len = line.len();
        let newline_cost = usize::from(!output_lines_rev.is_empty());
        let additional_bytes = line_len.saturating_add(newline_cost);

        if output_bytes_count.saturating_add(additional_bytes) > max_bytes {
            truncated_by = TruncatedBy::Bytes;
            if output_lines_rev.is_empty() {
                let partial = truncate_string_to_bytes_from_end(line, max_bytes);
                output_lines_rev.push(partial);
                last_line_partial = true;
            }
            break;
        }

        output_lines_rev.push(*line);
        output_bytes_count = output_bytes_count.saturating_add(additional_bytes);

        if output_lines_rev.len() >= max_lines {
            truncated_by = TruncatedBy::Lines;
            break;
        }
    }

    output_lines_rev.reverse();
    let content = output_lines_rev.join("\n");
    Truncation {
        output_bytes: content.len(),
        output_lines: output_lines_rev.len(),
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
