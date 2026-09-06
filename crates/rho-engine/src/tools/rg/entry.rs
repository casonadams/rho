use crate::tools::truncate::{DEFAULT_MAX_BYTES, GREP_MAX_LINE_LENGTH, format_size, truncate_head};
use crate::tools::types::ToolResult;

pub const RG_COLLECTION_CEILING: usize = 5_000;

#[derive(Debug, Clone)]
pub struct LineMatch {
    pub path: String,
    pub line: u64,
    pub text: String,
    pub truncated: bool,
}

pub fn render(matches: &[LineMatch]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(matches.len() * 64);
    for (i, m) in matches.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let _ = write!(out, "{}:{}: {}", m.path, m.line, m.text);
    }
    out
}

fn rg_limit_notice(total: usize, limit: usize) -> String {
    if total >= RG_COLLECTION_CEILING {
        format!(
            "showing first {limit} of {RG_COLLECTION_CEILING}+ matches (collection ceiling reached); narrow with a tighter pattern, path, or type"
        )
    } else {
        format!("showing first {limit} of {total} matches; narrow with a tighter pattern, path, or type")
    }
}

fn collect_rg_notices(total: usize, limit: usize, (has_bytes, has_lines): (bool, bool)) -> Vec<String> {
    let mut notices = Vec::new();
    if total > limit {
        notices.push(rg_limit_notice(total, limit));
    }
    if has_bytes {
        notices.push(format!("{} limit reached", format_size(DEFAULT_MAX_BYTES)));
    }
    if has_lines {
        notices.push(format!(
            "Some lines truncated to {GREP_MAX_LINE_LENGTH} chars. Use read tool to see full lines"
        ));
    }
    notices
}

pub fn format_results(mut matches: Vec<LineMatch>, limit: usize) -> ToolResult {
    if matches.is_empty() {
        return ToolResult::success("No matches found");
    }
    matches.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    let total = matches.len();
    if total > limit {
        matches.truncate(limit);
    }
    let lines_truncated = matches.iter().any(|m| m.truncated);
    let rendered = render(&matches);
    let truncation = truncate_head(&rendered, usize::MAX, DEFAULT_MAX_BYTES);
    let notices = collect_rg_notices(total, limit, (truncation.truncated_by.is_some(), lines_truncated));
    let mut output = truncation.content;
    if !notices.is_empty() {
        output.push_str(&format!("\n\n[{}]", notices.join(". ")));
    }
    ToolResult::success(output)
}
