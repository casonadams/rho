pub const DEFAULT_MAX_LINES: usize = 2000;
pub const DEFAULT_MAX_BYTES: usize = 50 * 1024; // 50 KB

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TruncatedBy {
    Lines,
    Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailTruncation {
    pub content: String,
    pub truncated: bool,
    pub truncated_by: Option<TruncatedBy>,
    pub total_lines: usize,
    pub total_bytes: usize,
    pub output_lines: usize,
    pub output_bytes: usize,
    pub last_line_partial: bool,
    pub max_lines: usize,
    pub max_bytes: usize,
}

pub fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

pub fn truncate_tail(content: &str, max_lines: usize, max_bytes: usize) -> TailTruncation {
    let total_bytes = content.len();
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    if total_lines <= max_lines && total_bytes <= max_bytes {
        return TailTruncation {
            content: content.to_string(),
            truncated: false,
            truncated_by: None,
            total_lines,
            total_bytes,
            output_lines: total_lines,
            output_bytes: total_bytes,
            last_line_partial: false,
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
    let output_content = output_lines_rev.join("\n");
    let final_output_bytes = output_content.len();
    let final_output_lines = output_lines_rev.len();

    TailTruncation {
        content: output_content,
        truncated: true,
        truncated_by: Some(truncated_by),
        total_lines,
        total_bytes,
        output_lines: final_output_lines,
        output_bytes: final_output_bytes,
        last_line_partial,
        max_lines,
        max_bytes,
    }
}

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
