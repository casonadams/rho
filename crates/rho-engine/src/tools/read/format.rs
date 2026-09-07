use crate::tools::truncate::{DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES, TruncatedBy, format_size, truncate_head};
use crate::tools::types::ToolResult;
use rho_harness_core::args::ReadArgs;

fn build_truncation_notice(truncated_by: TruncatedBy, start: usize, end: usize, total: usize) -> String {
    let next = end + 1;
    match truncated_by {
        TruncatedBy::Lines => format!("\n\n[Showing lines {start}-{end} of {total}. Use offset={next} to continue.]"),
        TruncatedBy::Bytes => format!(
            "\n\n[Showing lines {start}-{end} of {total} ({} limit). Use offset={next} to continue.]",
            format_size(DEFAULT_MAX_BYTES)
        ),
    }
}

fn build_user_limit_notice(start: usize, limit: usize, total: usize) -> Option<String> {
    let remaining = total.saturating_sub(start + limit);
    if remaining > 0 {
        let next = start + limit + 1;
        Some(format!(
            "\n\n[{remaining} more lines in file. Use offset={next} to continue.]"
        ))
    } else {
        None
    }
}

fn check_first_line_oversized(first_line: &str, start_line: usize, clean_path: &str) -> ToolResult {
    ToolResult::success(format!(
        "[Line {start_line} is {}, exceeds {} limit. Use bash: sed -n '{start_line}p' {clean_path} | head -c {DEFAULT_MAX_BYTES}]",
        format_size(first_line.len()),
        format_size(DEFAULT_MAX_BYTES),
    ))
}

fn slice_content(content: &str, start_idx: usize, limit: Option<usize>) -> String {
    let selected_iter = content.lines().skip(start_idx);
    match limit {
        Some(l) => selected_iter.take(l).collect::<Vec<_>>().join("\n"),
        None => selected_iter.collect::<Vec<_>>().join("\n"),
    }
}

pub fn format_content(content: &str, clean_path: &str, args: &ReadArgs) -> ToolResult {
    let offset = args.offset.unwrap_or(1).max(1);
    let total_lines = content.lines().count();
    let start_idx = offset.saturating_sub(1);
    if start_idx >= total_lines {
        return ToolResult::error(format!(
            "Offset {offset} is beyond end of file ({total_lines} lines total)"
        ));
    }
    let selected = slice_content(content, start_idx, args.limit);
    let truncation = truncate_head(&selected, DEFAULT_MAX_LINES, DEFAULT_MAX_BYTES);
    let start_line = start_idx + 1;
    if truncation.first_line_exceeds_limit {
        let first_line = content.lines().nth(start_idx).unwrap_or("");
        return check_first_line_oversized(first_line, start_line, clean_path);
    }
    let mut output = number_lines(&truncation.content, start_line);
    if let Some(by) = truncation.truncated_by {
        let end_line = start_line + truncation.output_lines - 1;
        output.push_str(&build_truncation_notice(by, start_line, end_line, total_lines));
    } else if let Some(limit) = args.limit
        && let Some(notice) = build_user_limit_notice(start_idx, limit, total_lines)
    {
        output.push_str(&notice);
    }
    ToolResult::success(output)
}

pub fn number_lines(content: &str, start_line: usize) -> String {
    use std::fmt::Write;
    let line_count = content.lines().count();
    let mut output = String::with_capacity(content.len() + line_count * 8);
    for (idx, line) in content.lines().enumerate() {
        let line_num = start_line + idx;
        let _ = writeln!(output, "{line_num:6}\t{line}");
    }
    output
}

pub fn is_binary(bytes: &[u8]) -> bool {
    let check_len = bytes.len().min(1024);
    bytes[..check_len].contains(&0)
}
