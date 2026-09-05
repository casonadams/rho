use std::path::Path;

use rho_harness_core::workspace::Workspace;

use super::format::{FileOutline, format_outlines};
use super::grammar::SupportedLanguage;
use super::parser::parse_symbols;
use super::types::SymbolEntry;
use crate::tools::traversal::{search_root, walker_builder};
use crate::tools::types::ToolResult;

pub const MAX_SCAN_FILES: usize = 500;
pub const DEFAULT_MAX_DEPTH: usize = 2;
pub const MAX_DEPTH: usize = 5;

pub struct OutlineSearchOptions<'a> {
    pub path: &'a str,
    pub query: Option<&'a str>,
    pub kind: Option<&'a str>,
    pub depth: Option<usize>,
}

impl OutlineSearchOptions<'_> {
    pub fn max_depth(&self) -> usize {
        self.depth.unwrap_or(DEFAULT_MAX_DEPTH).min(MAX_DEPTH)
    }

    pub fn matches(&self, sym: &SymbolEntry) -> bool {
        if sym.depth > self.max_depth() {
            return false;
        }
        if self.kind.is_some_and(|k| !sym.kind.matches(k)) {
            return false;
        }
        if let Some(q) = self.query.map(str::trim)
            && !q.is_empty()
            && !sym.name.to_lowercase().contains(&q.to_lowercase())
        {
            return false;
        }
        true
    }
}

pub fn search_outline(workspace: &Workspace, options: OutlineSearchOptions<'_>) -> Result<ToolResult, String> {
    let root = search_root(workspace, Some(options.path))?;

    if root.is_file() {
        return outline_single_file(workspace, &root, &options);
    }

    outline_directory(workspace, &root, &options)
}

fn outline_single_file(
    workspace: &Workspace,
    file_path: &Path,
    options: &OutlineSearchOptions<'_>,
) -> Result<ToolResult, String> {
    let Some(lang) = SupportedLanguage::from_path(file_path) else {
        return Ok(ToolResult::error(unsupported_message(file_path)));
    };

    let content = std::fs::read_to_string(file_path).map_err(|e| format!("Failed to read file: {e}"))?;
    let symbols = parse_symbols(&content, lang).map_err(|e| format!("Failed to parse symbols: {e}"))?;

    let filtered: Vec<_> = symbols.into_iter().filter(|s| options.matches(s)).collect();

    let rel = file_path
        .strip_prefix(workspace.root())
        .unwrap_or(file_path)
        .to_string_lossy()
        .to_string();

    Ok(format_outlines(
        &[FileOutline {
            path: rel,
            symbols: filtered,
        }],
        false,
    ))
}

fn outline_directory(
    workspace: &Workspace,
    dir_path: &Path,
    options: &OutlineSearchOptions<'_>,
) -> Result<ToolResult, String> {
    let mut outlines = Vec::new();
    let mut scanned_files = 0;
    let mut hit_file_limit = false;

    for entry in walker_builder(dir_path, false).build().flatten() {
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();
        let Some(lang) = SupportedLanguage::from_path(path) else {
            continue;
        };
        scanned_files += 1;
        if scanned_files > MAX_SCAN_FILES {
            hit_file_limit = true;
            break;
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(symbols) = parse_symbols(&content, lang) else {
            continue;
        };
        let filtered: Vec<_> = symbols.into_iter().filter(|s| options.matches(s)).collect();
        if !filtered.is_empty() {
            let rel = path
                .strip_prefix(workspace.root())
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();
            outlines.push(FileOutline {
                path: rel,
                symbols: filtered,
            });
        }
    }

    outlines.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(format_outlines(&outlines, hit_file_limit))
}

fn unsupported_message(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => format!("Syntax outline not supported for extension '.{ext}'. Use 'read' or 'rg' instead."),
        None => "Syntax outline not supported for file without extension. Use 'read' or 'rg' instead.".to_string(),
    }
}

#[cfg(test)]
#[path = "search/tests.rs"]
mod tests;
