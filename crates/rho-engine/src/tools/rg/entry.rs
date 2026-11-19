use std::path::Path;

use crate::tools::truncate::{GREP_MAX_LINE_LENGTH, finish_listing, listing_limit_notice};
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
        }
        let _ = writeln!(out, "{}:{}: {}", m.path, m.line, m.text);
    }
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

pub fn format_results(mut matches: Vec<LineMatch>, limit: usize, artifact_dir: Option<&Path>) -> ToolResult {
    if matches.is_empty() {
        return ToolResult::success("No matches found");
    }
    matches.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    let total = matches.len();
    let ceiling = (total >= RG_COLLECTION_CEILING).then_some(RG_COLLECTION_CEILING);
    let limit_notice = (total > limit).then(|| listing_limit_notice(limit, total, ceiling));
    matches.truncate(limit);
    let line_notice = matches
        .iter()
        .any(|m| m.truncated)
        .then(|| format!("Some lines truncated to {GREP_MAX_LINE_LENGTH} chars. Use read tool to see full lines"));
    finish_listing(&render(&matches), limit_notice, line_notice, artifact_dir)
}
