use crate::tools::truncate::{DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES, TruncatedBy, format_size, truncate_head};
use crate::tools::types::ToolResult;
use rho_harness_core::args::ReadArgs;

/// pi's read-tool text branch: slice from the 1-indexed offset, truncate the
/// selection with the shared head truncator, then assemble numbered output
/// with actionable continuation notices.
pub fn format_content(content: &str, clean_path: &str, args: &ReadArgs) -> ToolResult {
    let offset = args.offset.unwrap_or(1).max(1);
    let user_limit = args.limit;
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();
    let start_idx = offset.saturating_sub(1);

    if start_idx >= total_lines {
        return ToolResult::error(format!(
            "Offset {offset} is beyond end of file ({total_lines} lines total)"
        ));
    }

    let selected = match user_limit {
        Some(limit) => lines[start_idx..(start_idx + limit).min(total_lines)].join("\n"),
        None => lines[start_idx..].join("\n"),
    };

    let truncation = truncate_head(&selected, DEFAULT_MAX_LINES, DEFAULT_MAX_BYTES);
    let start_line = start_idx + 1;

    if truncation.first_line_exceeds_limit {
        return ToolResult::success(format!(
            "[Line {start_line} is {}, exceeds {} limit. Use bash: sed -n '{start_line}p' {clean_path} | head -c {DEFAULT_MAX_BYTES}]",
            format_size(lines[start_idx].len()),
            format_size(DEFAULT_MAX_BYTES),
        ));
    }

    let mut output = number_lines(&truncation.content, start_line);

    if let Some(truncated_by) = truncation.truncated_by {
        let end_line = start_line + truncation.output_lines - 1;
        let next_offset = end_line + 1;
        match truncated_by {
            TruncatedBy::Lines => output.push_str(&format!(
                "\n\n[Showing lines {start_line}-{end_line} of {total_lines}. Use offset={next_offset} to continue.]"
            )),
            TruncatedBy::Bytes => output.push_str(&format!(
                "\n\n[Showing lines {start_line}-{end_line} of {total_lines} ({} limit). Use offset={next_offset} to continue.]",
                format_size(DEFAULT_MAX_BYTES)
            )),
        }
    } else if let Some(limit) = user_limit {
        let remaining = total_lines.saturating_sub(start_idx + limit);
        if remaining > 0 {
            let next_offset = start_idx + limit + 1;
            output.push_str(&format!(
                "\n\n[{remaining} more lines in file. Use offset={next_offset} to continue.]"
            ));
        }
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
