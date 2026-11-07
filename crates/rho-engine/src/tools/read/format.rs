use std::path::Path;

use crate::tools::truncate::{
    DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES, TruncatedBy, format_size, split_artifact_notice, truncate_head_with_spill,
};
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

pub fn format_content(content: &str, clean_path: &str, args: &ReadArgs, artifact_dir: Option<&Path>) -> ToolResult {
    let offset = args.offset.unwrap_or(1).max(1);
    let total_lines = content.lines().count();
    let start_idx = offset.saturating_sub(1);
    if start_idx >= total_lines {
        return ToolResult::error(format!(
            "Offset {offset} is beyond end of file ({total_lines} lines total)"
        ));
    }
    let selected = slice_content(content, start_idx, args.limit);
    let truncation = truncate_head_with_spill(&selected, DEFAULT_MAX_LINES, DEFAULT_MAX_BYTES, artifact_dir);
    let start_line = start_idx + 1;
    if truncation.first_line_exceeds_limit {
        let first_line = content.lines().nth(start_idx).unwrap_or("");
        return check_first_line_oversized(first_line, start_line, clean_path);
    }
    let (text, spilled_notice) = split_artifact_notice(&truncation.content);
    let mut output = number_lines(text, start_line);
    if let Some(by) = truncation.truncated_by {
        let end_line = start_line + truncation.output_lines - 1;
        output.push_str(&build_truncation_notice(by, start_line, end_line, total_lines));
    } else if let Some(limit) = args.limit
        && let Some(notice) = build_user_limit_notice(start_idx, limit, total_lines)
    {
        output.push_str(&notice);
    }
    if let Some(spill) = spilled_notice {
        output.push_str("\n\n");
        output.push_str(spill);
    }
    ToolResult::success(output)
}

pub fn number_lines(content: &str, start_line: usize) -> String {
    use std::fmt::Write;
    let line_count = content.lines().count();
    let max_line = start_line.saturating_add(line_count);
    let width = max_line.to_string().len().max(3);
    let mut output = String::with_capacity(content.len() + line_count * (width + 2));
    for (idx, line) in content.lines().enumerate() {
        let line_num = start_line + idx;
        let _ = writeln!(output, "{line_num:>width$}\t{line}");
    }
    output
}

pub fn is_binary(bytes: &[u8]) -> bool {
    let check_len = bytes.len().min(1024);
    bytes[..check_len].contains(&0)
}
