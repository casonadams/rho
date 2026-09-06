use super::types::SymbolEntry;
use crate::tools::truncate::{DEFAULT_MAX_BYTES, format_size, truncate_head};
use crate::tools::types::ToolResult;

#[derive(Debug, Clone)]
pub struct FileOutline {
    pub path: String,
    pub symbols: Vec<SymbolEntry>,
}

fn format_file_outline(file: &FileOutline) -> Option<String> {
    if file.symbols.is_empty() {
        return None;
    }
    let mut lines = Vec::with_capacity(file.symbols.len() + 1);
    lines.push(format!("{}:", file.path));
    for sym in &file.symbols {
        let indent = 2 + sym.depth * 2;
        lines.push(format!(
            "{:indent$}line {}: {}",
            "",
            sym.line,
            sym.signature,
            indent = indent
        ));
    }
    Some(lines.join("\n"))
}

fn collect_outline_notices(hit_file_limit: bool, has_truncation: bool) -> Vec<String> {
    let mut notices = Vec::new();
    if hit_file_limit {
        notices.push("scanned 500 files limit reached; narrow with a more specific path or query".to_string());
    }
    if has_truncation {
        notices.push(format!("{} limit reached", format_size(DEFAULT_MAX_BYTES)));
    }
    notices
}

pub fn format_outlines(outlines: &[FileOutline], hit_file_limit: bool) -> ToolResult {
    let blocks: Vec<String> = outlines.iter().filter_map(format_file_outline).collect();
    if blocks.is_empty() {
        return ToolResult::success("No matching symbols found");
    }

    let rendered = blocks.join("\n\n");
    let truncation = truncate_head(&rendered, usize::MAX, DEFAULT_MAX_BYTES);
    let notices = collect_outline_notices(hit_file_limit, truncation.truncated_by.is_some());
    let mut output = truncation.content;
    if !notices.is_empty() {
        output.push_str(&format!("\n\n[{}]", notices.join(". ")));
    }

    ToolResult::success(output)
}

#[cfg(test)]
#[path = "format/tests.rs"]
mod tests;
