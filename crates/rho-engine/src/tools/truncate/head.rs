use super::{TruncatedBy, Truncation};

/// Truncate content from the head (keep the first N lines/bytes). Suitable for
/// file reads where the beginning matters.
pub fn truncate_head(content: &str, max_lines: usize, max_bytes: usize) -> Truncation {
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

    // The first line alone exceeds the byte limit: emit nothing and let the
    // caller point the model at a bash fallback.
    if lines[0].len() > max_bytes {
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

    let mut kept: Vec<&str> = Vec::new();
    let mut output_bytes = 0usize;
    let mut truncated_by = TruncatedBy::Lines;

    for (i, line) in lines.iter().enumerate().take(max_lines) {
        // +1 for the joining newline, which the first line does not cost.
        let line_bytes = line.len() + usize::from(i > 0);
        if output_bytes + line_bytes > max_bytes {
            truncated_by = TruncatedBy::Bytes;
            break;
        }
        kept.push(line);
        output_bytes += line_bytes;
    }

    if kept.len() >= max_lines && output_bytes <= max_bytes {
        truncated_by = TruncatedBy::Lines;
    }

    let content = kept.join("\n");
    Truncation {
        output_bytes: content.len(),
        output_lines: kept.len(),
        content,
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
