use crate::tools::types::ToolResult;

pub struct FormatFetchParams<'a> {
    pub text: &'a str,
    pub offset: usize,
    pub limit: usize,
    pub url_str: &'a str,
}

pub fn format_fetch_output(params: FormatFetchParams<'_>) -> ToolResult {
    let lines: Vec<&str> = params.text.lines().collect();
    let total_lines = lines.len();

    if total_lines == 0 {
        return ToolResult::success("[Empty content returned from URL]");
    }

    let start_idx = (params.offset - 1).min(total_lines);
    let end_idx = (start_idx + params.limit).min(total_lines);

    let mut output = String::new();
    for (i, line) in lines[start_idx..end_idx].iter().enumerate() {
        let line_num = start_idx + i + 1;
        output.push_str(&format!("{line_num:5}\t{line}\n"));
    }

    if end_idx < total_lines {
        output.push_str(&format!(
            "\n[Lines {}-{} of {} total lines from {}]",
            params.offset, end_idx, total_lines, params.url_str
        ));
    }

    ToolResult::success(output)
}
