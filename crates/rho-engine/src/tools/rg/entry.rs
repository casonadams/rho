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
    matches
        .iter()
        // pi's grep line format: `path:line: text` with a space before the text.
        .map(|m| format!("{}:{}: {}", m.path, m.line, m.text))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn format_results(mut matches: Vec<LineMatch>, limit: usize) -> ToolResult {
    if matches.is_empty() {
        return ToolResult::success("No matches found");
    }
    // Sort before truncating so parallel-walk collection order never leaks into output.
    matches.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    let total = matches.len();
    let mut notices: Vec<String> = Vec::new();
    if total > limit {
        notices.push(if total >= RG_COLLECTION_CEILING {
            format!(
                "showing first {limit} of {RG_COLLECTION_CEILING}+ matches (collection ceiling reached); narrow with a tighter pattern, path, or type"
            )
        } else {
            format!("showing first {limit} of {total} matches; narrow with a tighter pattern, path, or type")
        });
        matches.truncate(limit);
    }
    // pi tracks line truncation over emitted rows only, so hidden matches
    // never claim the notice.
    let lines_truncated = matches.iter().any(|m| m.truncated);
    let rendered = render(&matches);
    // pi caps grep output by bytes only; the match limit already caps rows.
    let truncation = truncate_head(&rendered, usize::MAX, DEFAULT_MAX_BYTES);
    if truncation.truncated_by.is_some() {
        notices.push(format!("{} limit reached", format_size(DEFAULT_MAX_BYTES)));
    }
    let mut output = truncation.content;
    if lines_truncated {
        notices.push(format!(
            "Some lines truncated to {GREP_MAX_LINE_LENGTH} chars. Use read tool to see full lines"
        ));
    }
    if !notices.is_empty() {
        output.push_str(&format!("\n\n[{}]", notices.join(". ")));
    }
    ToolResult::success(output)
}
