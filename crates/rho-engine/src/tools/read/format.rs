use crate::tools::truncate::{DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES, TruncatedBy, format_size, truncate_head};
use crate::tools::types::ToolResult;
use rho_harness_core::args::ReadArgs;

/// pi's read-tool text branch: slice from the 1-indexed offset, truncate the
/// selection with the shared head truncator, then assemble numbered output
/// with actionable continuation notices.
fn select_lines(lines: &[&str], start_idx: usize, user_limit: Option<usize>) -> String {
    match user_limit {
        Some(limit) => lines[start_idx..(start_idx + limit).min(lines.len())].join("\n"),
        None => lines[start_idx..].join("\n"),
    }
}

fn build_truncation_notice(truncated_by: TruncatedBy, (start, end, total): (usize, usize, usize)) -> String {
    let next = end + 1;
    match truncated_by {
        TruncatedBy::Lines => format!("\n\n[Showing lines {start}-{end} of {total}. Use offset={next} to continue.]"),
        TruncatedBy::Bytes => format!(
            "\n\n[Showing lines {start}-{end} of {total} ({} limit). Use offset={next} to continue.]",
            format_size(DEFAULT_MAX_BYTES)
        ),
    }
}

fn build_user_limit_notice((start, limit, total): (usize, usize, usize)) -> Option<String> {
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

fn check_first_line_oversized(first_line: &str, (start_line, clean_path): (usize, &str)) -> ToolResult {
    ToolResult::success(format!(
        "[Line {start_line} is {}, exceeds {} limit. Use bash: sed -n '{start_line}p' {clean_path} | head -c {DEFAULT_MAX_BYTES}]",
        format_size(first_line.len()),
        format_size(DEFAULT_MAX_BYTES),
    ))
}

pub fn format_content(content: &str, clean_path: &str, args: &ReadArgs) -> ToolResult {
    let offset = args.offset.unwrap_or(1).max(1);
    let lines: Vec<&str> = content.lines().collect();
    let start_idx = offset.saturating_sub(1);
    if start_idx >= lines.len() {
        return ToolResult::error(format!(
            "Offset {offset} is beyond end of file ({} lines total)",
            lines.len()
        ));
    }
    let selected = select_lines(&lines, start_idx, args.limit);
    let truncation = truncate_head(&selected, DEFAULT_MAX_LINES, DEFAULT_MAX_BYTES);
    let start_line = start_idx + 1;
    if truncation.first_line_exceeds_limit {
        return check_first_line_oversized(lines[start_idx], (start_line, clean_path));
    }
    let mut output = number_lines(&truncation.content, start_line);
    if let Some(by) = truncation.truncated_by {
        let end_line = start_line + truncation.output_lines - 1;
        output.push_str(&build_truncation_notice(by, (start_line, end_line, lines.len())));
    } else if let Some(limit) = args.limit
        && let Some(notice) = build_user_limit_notice((start_idx, limit, lines.len()))
    {
        output.push_str(&notice);
    }
    ToolResult::success(output)
}

pub fn number_lines(content: &str, start_line: usize) -> String {
    let mut output = String::new();
    for (idx, line) in content.lines().enumerate() {
        let line_num = start_line + idx;
        output.push_str(&format!("{line_num:6}\t{line}\n"));
    }
    output
}

pub fn is_binary(bytes: &[u8]) -> bool {
    let check_len = bytes.len().min(1024);
    bytes[..check_len].contains(&0)
}
