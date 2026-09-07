use crate::tools::types::ToolResult;

pub const PAGE_BYTE_LIMIT: usize = 44_744;
pub const LINE_MAX_BYTES: usize = 4_000;

pub struct PageResult {
    pub content: String,
    pub consumed: usize,
}

pub struct PageLinesParams<'a> {
    pub lines: &'a [String],
    pub start: usize,
    pub line_limit: usize,
    pub byte_limit: usize,
}

fn split_oversized_line(line: &str, max_bytes: usize, output: &mut Vec<String>) {
    let mut current = String::new();
    for ch in line.chars() {
        if !current.is_empty() && current.len() + ch.len_utf8() > max_bytes {
            output.push(current);
            current = String::new();
        }
        current.push(ch);
    }
    if !current.is_empty() {
        output.push(current);
    }
}

pub fn prepare_lines(text: &str, max_bytes: usize) -> Vec<String> {
    let mut output = Vec::new();
    for line in text.split('\n') {
        if line.len() <= max_bytes {
            output.push(line.to_string());
        } else {
            split_oversized_line(line, max_bytes, &mut output);
        }
    }
    output
}

pub fn page_lines(params: PageLinesParams<'_>) -> PageResult {
    let mut selected: Vec<&str> = Vec::new();
    let mut bytes = 0;
    let end = params.lines.len().min(params.start + params.line_limit);
    for line in &params.lines[params.start..end] {
        let line_bytes = line.len() + usize::from(!selected.is_empty());
        if !selected.is_empty() && bytes + line_bytes > params.byte_limit {
            break;
        }
        selected.push(line.as_str());
        bytes += line_bytes;
    }
    PageResult {
        content: selected.join("\n"),
        consumed: selected.len(),
    }
}

pub struct FormatPageParams<'a> {
    pub text: &'a str,
    pub offset: usize,
    pub limit: usize,
    pub source_url: &'a str,
    pub final_url: &'a str,
}

fn assemble_output(source_url: &str, final_url: &str, text: &str) -> String {
    let mut out = String::new();
    if final_url != source_url {
        out.push_str(&format!("[Final URL after redirects: {final_url}]\n\n"));
    }
    out.push_str(text);
    out
}

pub fn format_page(params: FormatPageParams<'_>) -> ToolResult {
    let full_output = assemble_output(params.source_url, params.final_url, params.text);
    if full_output.trim().is_empty() {
        return ToolResult::success("[Empty content returned from URL]");
    }

    let prepared = prepare_lines(&full_output, LINE_MAX_BYTES);
    let total = prepared.len();
    let start = (params.offset.saturating_sub(1)).min(total);
    let page = page_lines(PageLinesParams {
        lines: &prepared,
        start,
        line_limit: params.limit.max(1),
        byte_limit: PAGE_BYTE_LIMIT,
    });
    let next_offset = start + page.consumed + 1;
    let content = if start + page.consumed < total {
        format!(
            "{}\n\n[Truncated: {total} lines total. Use offset={next_offset} to continue.]",
            page.content
        )
    } else {
        page.content
    };

    ToolResult::success(content)
}
