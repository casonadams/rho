use std::fmt::Write;

use super::types::SymbolEntry;
use crate::tools::truncate::{DEFAULT_MAX_BYTES, format_size, truncate_head};
use crate::tools::types::ToolResult;

#[derive(Debug, Clone)]
pub struct FileOutline {
    pub path: String,
    pub symbols: Vec<SymbolEntry>,
}

fn render_file_outline(out: &mut String, file: &FileOutline) {
    let _ = writeln!(out, "{}:", file.path);
    for sym in &file.symbols {
        let indent = 2 + sym.depth * 2;
        let _ = writeln!(
            out,
            "{:indent$}line {}: {}",
            "",
            sym.line,
            sym.signature,
            indent = indent
        );
    }
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

fn render_outlines(active_files: &[&FileOutline]) -> String {
    let mut rendered = String::with_capacity(active_files.len() * 256);
    for (i, file) in active_files.iter().enumerate() {
        if i > 0 {
            rendered.push('\n');
        }
        render_file_outline(&mut rendered, file);
    }
    if rendered.ends_with('\n') {
        rendered.pop();
    }
    rendered
}

pub fn format_outlines(outlines: &[FileOutline], hit_file_limit: bool) -> ToolResult {
    let active_files: Vec<&FileOutline> = outlines.iter().filter(|f| !f.symbols.is_empty()).collect();
    if active_files.is_empty() {
        return ToolResult::success("No matching symbols found");
    }

    let rendered = render_outlines(&active_files);
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
