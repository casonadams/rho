use std::path::Path;

use crate::tools::truncate::{
    DEFAULT_MAX_BYTES, GREP_MAX_LINE_LENGTH, format_size, split_artifact_notice, truncate_head_with_spill,
};
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
    let mut out = String::with_capacity(matches.len() * 48);
    let mut current_path: Option<&str> = None;
    for m in matches {
        if current_path != Some(&m.path) {
            if current_path.is_some() {
                out.push('\n');
            }
            current_path = Some(&m.path);
            let _ = writeln!(out, "{}:", m.path);
        }
        let _ = writeln!(out, "  {}: {}", m.line, m.text);
    }
    if out.ends_with('\n') {
        out.pop();
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

fn collect_rg_notices(total: usize, limit: usize, has_bytes: bool, has_lines: bool) -> Vec<String> {
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

pub fn format_results(mut matches: Vec<LineMatch>, limit: usize, artifact_dir: Option<&Path>) -> ToolResult {
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
    let truncation = truncate_head_with_spill(&rendered, usize::MAX, DEFAULT_MAX_BYTES, artifact_dir);
    let notices = collect_rg_notices(total, limit, truncation.truncated_by.is_some(), lines_truncated);
    let (body, spill_notice) = split_artifact_notice(&truncation.content);
    let mut output = body.to_string();
    if !notices.is_empty() {
        output.push_str(&format!("\n\n[{}]", notices.join(". ")));
    }
    if let Some(notice) = spill_notice {
        output.push_str("\n\n");
        output.push_str(notice);
    }
    ToolResult::success(output)
}
