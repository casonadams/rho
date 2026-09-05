use super::types::SymbolEntry;
use crate::tools::truncate::{DEFAULT_MAX_BYTES, format_size, truncate_head};
use crate::tools::types::ToolResult;

#[derive(Debug, Clone)]
pub struct FileOutline {
    pub path: String,
    pub symbols: Vec<SymbolEntry>,
}

pub fn format_outlines(outlines: &[FileOutline], hit_file_limit: bool) -> ToolResult {
    let mut blocks = Vec::new();
    for file in outlines {
        if file.symbols.is_empty() {
            continue;
        }
        let mut lines = Vec::new();
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
        blocks.push(lines.join("\n"));
    }

    if blocks.is_empty() {
        return ToolResult::success("No matching symbols found");
    }

    let rendered = blocks.join("\n\n");
    let truncation = truncate_head(&rendered, usize::MAX, DEFAULT_MAX_BYTES);

    let mut notices = Vec::new();
    if hit_file_limit {
        notices.push("scanned 500 files limit reached; narrow with a more specific path or query".to_string());
    }
    if truncation.truncated_by.is_some() {
        notices.push(format!("{} limit reached", format_size(DEFAULT_MAX_BYTES)));
    }

    let mut output = truncation.content;
    if !notices.is_empty() {
        output.push_str(&format!("\n\n[{}]", notices.join(". ")));
    }

    ToolResult::success(output)
}

#[cfg(test)]
#[path = "format/tests.rs"]
mod tests;
