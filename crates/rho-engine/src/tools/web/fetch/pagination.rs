use crate::tools::types::ToolResult;

pub(crate) const PAGE_BYTE_LIMIT: usize = 44_744;
pub(crate) const LINE_MAX_BYTES: usize = 4_000;

pub(crate) struct PageResult {
    pub content: String,
    pub consumed: usize,
}

pub(crate) struct PageLinesParams<'a> {
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

pub(crate) fn prepare_lines(text: &str, max_bytes: usize) -> Vec<String> {
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

pub(crate) fn page_lines(params: PageLinesParams<'_>) -> PageResult {
    let mut selected: Vec<&str> = Vec::new();
    let mut bytes = 0;
    let end = params.lines.len().min(params.start.saturating_add(params.line_limit));
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

pub(crate) struct FormatPageParams<'a> {
    pub text: &'a str,
    pub offset: usize,
    pub limit: usize,
    pub source_url: &'a str,
    pub final_url: &'a str,
}

fn redirect_header(source_url: &str, final_url: &str) -> String {
    if final_url == source_url {
        String::new()
    } else {
        format!("[Final URL after redirects: {final_url}]\n\n")
    }
}

pub(crate) fn format_page(params: FormatPageParams<'_>) -> ToolResult {
    let header = redirect_header(params.source_url, params.final_url);
    if params.text.trim().is_empty() {
        return ToolResult::success(format!("{header}[Empty content returned from URL]"));
    }

    let prepared = prepare_lines(params.text, LINE_MAX_BYTES);
    let total = prepared.len();
    let start = params.offset.saturating_sub(1);
    if start >= total {
        return ToolResult::error(format!(
            "Offset {} is beyond end of content ({total} lines total)",
            params.offset
        ));
    }
    let page = page_lines(PageLinesParams {
        lines: &prepared,
        start,
        line_limit: params.limit.max(1),
        byte_limit: PAGE_BYTE_LIMIT,
    });
    let next_offset = start + page.consumed + 1;
    let content = if start + page.consumed < total {
        format!(
            "{header}{}\n\n[Truncated: {total} lines total. Use offset={next_offset} to continue.]",
            page.content
        )
    } else {
        format!("{header}{}", page.content)
    };

    ToolResult::success(content)
}
