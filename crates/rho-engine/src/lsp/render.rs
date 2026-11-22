//! Rendering LSP responses as compact text for the model, and applying workspace edits.

use super::client::{Diagnostic, PositionEncoding, uri_path};
use super::position::byte_index;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};

pub const MAX_LOCATIONS: usize = 100;
pub const MAX_DIAGNOSTICS: usize = 20;
pub const MAX_SYMBOLS: usize = 200;
const MAX_MESSAGE_CHARS: usize = 300;

const SYMBOL_KINDS: [&str; 26] = [
    "file",
    "module",
    "namespace",
    "package",
    "class",
    "method",
    "property",
    "field",
    "constructor",
    "enum",
    "interface",
    "function",
    "variable",
    "constant",
    "string",
    "number",
    "boolean",
    "array",
    "object",
    "key",
    "null",
    "enum-member",
    "struct",
    "event",
    "operator",
    "type-parameter",
];

pub fn symbol_kind(value: &Value) -> &'static str {
    value
        .get("kind")
        .and_then(Value::as_u64)
        .and_then(|kind| SYMBOL_KINDS.get((kind as usize).wrapping_sub(1)))
        .copied()
        .unwrap_or("symbol")
}

pub fn display_path(workspace: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(workspace).map(Path::to_path_buf).ok().or_else(|| {
        let canonical = workspace.canonicalize().ok()?;
        path.strip_prefix(canonical).ok().map(Path::to_path_buf)
    });
    relative.as_deref().unwrap_or(path).display().to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Location {
    pub path: PathBuf,
    /// 1-based.
    pub line: u32,
    /// 0-based character in the server's encoding.
    pub character: u32,
}

fn location_from(value: &Value) -> Option<Location> {
    let uri = value.get("uri").or_else(|| value.get("targetUri"))?.as_str()?;
    let range = value
        .get("targetSelectionRange")
        .or_else(|| value.get("range"))
        .or_else(|| value.get("targetRange"))?;
    let start = range.get("start")?;
    Some(Location {
        path: uri_path(uri)?,
        line: start.get("line")?.as_u64()? as u32 + 1,
        character: start.get("character")?.as_u64()? as u32,
    })
}

/// Locations from a `Location`, `Location[]`, or `LocationLink[]` response, sorted and deduplicated.
pub fn locations(response: &Value) -> Vec<Location> {
    let mut found: Vec<Location> = match response {
        Value::Array(items) => items.iter().filter_map(location_from).collect(),
        Value::Object(_) => location_from(response).into_iter().collect(),
        _ => Vec::new(),
    };
    found.sort();
    found.dedup();
    found
}

fn file_lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|text| text.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

/// The location count, then each location grouped under its file as `LINE:text`, like `rg` output.
pub fn format_locations(workspace: &Path, noun: &str, found: &[Location]) -> String {
    if found.is_empty() {
        return format!("No {noun} found");
    }
    let mut out = crate::tools::listing::count_noun(found.len(), noun);
    let mut current: Option<&Path> = None;
    let mut lines = Vec::new();
    for location in found.iter().take(MAX_LOCATIONS) {
        if current != Some(location.path.as_path()) {
            current = Some(&location.path);
            lines = file_lines(&location.path);
            let _ = write!(out, "\n\n{}", display_path(workspace, &location.path));
        }
        let text = lines.get(location.line as usize - 1).map_or("", |line| line.trim());
        let _ = write!(
            out,
            "\n{}:{}",
            location.line,
            crate::tools::truncate::truncate_line(text).text
        );
    }
    if found.len() > MAX_LOCATIONS {
        let _ = write!(out, "\n\n[{} more not shown]", found.len() - MAX_LOCATIONS);
    }
    out
}

fn severity_name(severity: u8) -> &'static str {
    match severity {
        1 => "error",
        2 => "warning",
        3 => "info",
        _ => "hint",
    }
}

fn short_message(message: &str) -> String {
    let first = message.lines().next().unwrap_or("").trim();
    match first.char_indices().nth(MAX_MESSAGE_CHARS) {
        Some((cut, _)) => format!("{}...", &first[..cut]),
        None => first.to_string(),
    }
}

/// Errors and warnings first, then by position; deduplicated across servers.
pub fn sort_diagnostics(diagnostics: &mut Vec<Diagnostic>) {
    diagnostics
        .sort_by(|a, b| (a.severity, a.line, a.column, &a.message).cmp(&(b.severity, b.line, b.column, &b.message)));
    diagnostics.dedup_by(|a, b| a.line == b.line && a.column == b.column && a.message == b.message);
}

pub fn count_summary(diagnostics: &[Diagnostic]) -> String {
    let errors = diagnostics.iter().filter(|d| d.severity == 1).count();
    let warnings = diagnostics.iter().filter(|d| d.severity == 2).count();
    let others = diagnostics.len() - errors - warnings;
    let mut parts = vec![
        crate::tools::listing::count_noun(errors, "error"),
        crate::tools::listing::count_noun(warnings, "warning"),
    ];
    if others > 0 {
        parts.push(format!("{others} info/hint"));
    }
    parts.join(", ")
}

/// One `LINE:COL severity [source] message (code)` row per diagnostic, at most `MAX_DIAGNOSTICS`.
pub fn diagnostic_rows(diagnostics: &[Diagnostic]) -> String {
    let mut out = String::new();
    for d in diagnostics.iter().take(MAX_DIAGNOSTICS) {
        let source = d.source.as_deref().map(|s| format!(" [{s}]")).unwrap_or_default();
        let code = d.code.as_deref().map(|c| format!(" ({c})")).unwrap_or_default();
        let _ = writeln!(
            out,
            "{}:{} {}{source} {}{code}",
            d.line,
            d.column,
            severity_name(d.severity),
            short_message(&d.message)
        );
    }
    if diagnostics.len() > MAX_DIAGNOSTICS {
        let _ = writeln!(out, "[{} more not shown]", diagnostics.len() - MAX_DIAGNOSTICS);
    }
    out.truncate(out.trim_end().len());
    out
}

pub fn hover_text(response: &Value) -> String {
    let contents = response.get("contents").unwrap_or(&Value::Null);
    let text = match contents {
        Value::String(text) => text.clone(),
        Value::Object(map) => map.get("value").and_then(Value::as_str).unwrap_or("").to_string(),
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::String(text) => text.clone(),
                other => other.get("value").and_then(Value::as_str).unwrap_or("").to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    };
    let text = text.trim();
    if text.is_empty() {
        return "No hover information".into();
    }
    match text.char_indices().nth(4000) {
        Some((cut, _)) => format!("{}\n[hover text truncated]", &text[..cut]),
        None => text.to_string(),
    }
}

/// A symbol with its 1-based line range and its path of names from the outermost container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolEntry {
    pub names: Vec<String>,
    pub kind: &'static str,
    pub detail: Option<String>,
    pub start: u32,
    pub end: u32,
}

fn line_of(value: &Value, key: &str) -> Option<u32> {
    Some(value.get(key)?.get("line")?.as_u64()? as u32 + 1)
}

fn flatten_symbol(value: &Value, parents: &[String], out: &mut Vec<SymbolEntry>) {
    let Some(name) = value.get("name").and_then(Value::as_str) else {
        return;
    };
    let range = value
        .get("range")
        .or_else(|| value.get("location").and_then(|l| l.get("range")));
    let (Some(start), Some(end)) = (
        range.and_then(|r| line_of(r, "start")),
        range.and_then(|r| line_of(r, "end")),
    ) else {
        return;
    };
    let mut names = parents.to_vec();
    if parents.is_empty()
        && let Some(container) = value
            .get("containerName")
            .and_then(Value::as_str)
            .filter(|c| !c.is_empty())
    {
        names.push(container.to_string());
    }
    names.push(name.to_string());
    out.push(SymbolEntry {
        names: names.clone(),
        kind: symbol_kind(value),
        detail: value.get("detail").and_then(Value::as_str).map(str::to_string),
        start,
        end,
    });
    for child in value.get("children").and_then(Value::as_array).into_iter().flatten() {
        flatten_symbol(child, &names, out);
    }
}

/// Document symbols (hierarchical or flat) in document order.
pub fn document_symbols(response: &Value) -> Vec<SymbolEntry> {
    let mut out = Vec::new();
    for item in response.as_array().into_iter().flatten() {
        flatten_symbol(item, &[], &mut out);
    }
    out
}

pub fn format_outline(entries: &[SymbolEntry]) -> String {
    if entries.is_empty() {
        return "No symbols found".into();
    }
    let mut out = String::new();
    for entry in entries.iter().take(MAX_SYMBOLS) {
        let indent = "  ".repeat(entry.names.len().saturating_sub(1));
        let detail = entry
            .detail
            .as_deref()
            .map(|d| format!("  {}", short_message(d)))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "{}-{} {indent}{} {}{detail}",
            entry.start,
            entry.end,
            entry.kind,
            entry.names.last().map_or("", String::as_str)
        );
    }
    if entries.len() > MAX_SYMBOLS {
        let _ = writeln!(out, "[{} more not shown]", entries.len() - MAX_SYMBOLS);
    }
    out.truncate(out.trim_end().len());
    out
}

/// Splits `Type.method`, `Type::method`, or `Type/method` into names.
pub fn symbol_path(query: &str) -> Vec<&str> {
    query
        .split(['.', '/'])
        .flat_map(|part| part.split("::"))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect()
}

/// Symbols whose name path ends with `query`'s names (exact names, case-sensitive). An impl block
/// named `impl Trait for Type` answers to `Type` and `Trait`.
pub fn matching_symbols<'a>(entries: &'a [SymbolEntry], query: &str) -> Vec<&'a SymbolEntry> {
    let wanted = symbol_path(query);
    if wanted.is_empty() {
        return Vec::new();
    }
    entries
        .iter()
        .filter(|entry| {
            entry.names.len() >= wanted.len()
                && entry.names[entry.names.len() - wanted.len()..]
                    .iter()
                    .zip(&wanted)
                    .all(|(name, want)| name_aliases(name).any(|alias| alias == *want))
        })
        .collect()
}

fn name_aliases(name: &str) -> impl Iterator<Item = &str> {
    let bare = name.strip_prefix("impl ").unwrap_or(name);
    let (first, second) = match bare.split_once(" for ") {
        Some((trait_name, type_name)) => (trait_name, Some(type_name)),
        None => (bare, None),
    };
    [Some(name), Some(first), second]
        .into_iter()
        .flatten()
        .map(|alias| alias.split(['(', '<', ' ']).next().unwrap_or(alias).trim())
}

pub fn format_workspace_symbols(workspace: &Path, response: &Value) -> String {
    let mut rows = Vec::new();
    for item in response.as_array().into_iter().flatten() {
        let Some(location) = item.get("location").and_then(location_from) else {
            continue;
        };
        let name = item.get("name").and_then(Value::as_str).unwrap_or("?");
        let container = item
            .get("containerName")
            .and_then(Value::as_str)
            .filter(|c| !c.is_empty())
            .map(|c| format!(" (in {c})"))
            .unwrap_or_default();
        rows.push(format!(
            "{}:{} {} {name}{container}",
            display_path(workspace, &location.path),
            location.line,
            symbol_kind(item)
        ));
    }
    if rows.is_empty() {
        return "No symbols found".into();
    }
    let total = rows.len();
    let mut out = format!("{}\n\n", crate::tools::listing::count_noun(total, "symbol"));
    out.push_str(&rows.into_iter().take(MAX_SYMBOLS).collect::<Vec<_>>().join("\n"));
    if total > MAX_SYMBOLS {
        let _ = write!(out, "\n\n[{} more not shown]", total - MAX_SYMBOLS);
    }
    out
}

#[derive(Debug, Clone)]
struct TextEdit {
    start: (u32, u32),
    end: (u32, u32),
    new_text: String,
}

fn text_edit(value: &Value) -> Option<TextEdit> {
    let range = value.get("range")?;
    let point = |key: &str| -> Option<(u32, u32)> {
        let p = range.get(key)?;
        Some((p.get("line")?.as_u64()? as u32, p.get("character")?.as_u64()? as u32))
    };
    Some(TextEdit {
        start: point("start")?,
        end: point("end")?,
        new_text: value.get("newText")?.as_str()?.to_string(),
    })
}

/// The edits of a `WorkspaceEdit`, by file: `documentChanges` when present (the spec prefers it
/// over `changes`), else `changes`. File creation, renaming, and deletion are rejected.
pub fn workspace_edits(edit: &Value) -> Result<BTreeMap<PathBuf, Vec<Value>>, String> {
    let mut by_file: BTreeMap<PathBuf, Vec<Value>> = BTreeMap::new();
    if let Some(changes) = edit.get("documentChanges").and_then(Value::as_array) {
        for change in changes {
            let Some(uri) = change
                .get("textDocument")
                .and_then(|d| d.get("uri"))
                .and_then(Value::as_str)
            else {
                let kind = change.get("kind").and_then(Value::as_str).unwrap_or("unknown");
                return Err(format!(
                    "the server wants a file {kind} operation, which rho does not apply"
                ));
            };
            let path = uri_path(uri).ok_or_else(|| format!("unsupported URI {uri}"))?;
            by_file.entry(path).or_default().extend(
                change
                    .get("edits")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
            );
        }
        return Ok(by_file);
    }
    for (uri, edits) in edit.get("changes").and_then(Value::as_object).into_iter().flatten() {
        let path = uri_path(uri).ok_or_else(|| format!("unsupported URI {uri}"))?;
        by_file
            .entry(path)
            .or_default()
            .extend(edits.as_array().cloned().unwrap_or_default());
    }
    Ok(by_file)
}

fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect()
}

fn byte_offset(text: &str, starts: &[usize], (line, character): (u32, u32), encoding: PositionEncoding) -> usize {
    let Some(&start) = starts.get(line as usize) else {
        return text.len();
    };
    let end = starts.get(line as usize + 1).map_or(text.len(), |next| next - 1);
    let end = if text[start..end].ends_with('\r') { end - 1 } else { end };
    start + byte_index(&text[start..end], character, encoding)
}

/// Applies LSP text edits to `text`; edits are non-overlapping and relative to the original.
pub fn apply_text_edits(text: &str, edits: &[Value], encoding: PositionEncoding) -> Result<String, String> {
    let starts = line_starts(text);
    let mut spans = Vec::new();
    for (index, value) in edits.iter().enumerate() {
        let edit = text_edit(value).ok_or("malformed text edit")?;
        let from = byte_offset(text, &starts, edit.start, encoding);
        let to = byte_offset(text, &starts, edit.end, encoding);
        spans.push((from, to.max(from), index, edit.new_text));
    }
    spans.sort_by_key(|span| std::cmp::Reverse((span.0, span.2)));
    let mut out = text.to_string();
    let mut limit = usize::MAX;
    for (from, to, _, new_text) in spans {
        if to > limit {
            return Err("overlapping text edits".into());
        }
        out.replace_range(from..to, &new_text);
        limit = from;
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
