use std::collections::HashSet;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, QueryCursor};

use super::classify::{SymbolCandidate, classify_kind_and_name};
use super::grammar::SupportedLanguage;
use super::queries::query_for_language;
use super::signature::extract_signature;
use super::types::{OutlineParseError, SymbolEntry};

const CONTAINER_KINDS: &[&str] = &[
    "impl_item",
    "trait_item",
    "class_declaration",
    "class_definition",
    "interface_declaration",
    "function_item",
    "function_definition",
    "function_declaration",
    "method_definition",
];

pub fn parse_symbols(source: &str, language: SupportedLanguage) -> Result<Vec<SymbolEntry>, OutlineParseError> {
    let mut parser = language.create_parser()?;
    let tree = parser.parse(source, None).ok_or(OutlineParseError::FailedParse)?;
    let query = query_for_language(language)?;
    let capture_names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());

    let mut entries = Vec::new();
    let mut seen = HashSet::new();

    while let Some(m) = matches.next() {
        let (decl_node, name_node, tag) = resolve_capture(m.captures, capture_names);
        let Some(decl) = decl_node else { continue };
        if !seen.insert(decl.id()) {
            continue;
        }

        let candidate = SymbolCandidate { tag, decl, name_node };
        let (kind, name) = classify_kind_and_name(candidate, source);
        let (signature, line) = extract_signature(decl, source);
        let depth = compute_depth(decl);

        entries.push(SymbolEntry {
            name,
            kind,
            signature,
            line,
            depth,
        });
    }

    entries.sort_by_key(|e| (e.line, e.depth));
    Ok(entries)
}

fn resolve_capture<'a>(
    captures: &'a [tree_sitter::QueryCapture<'a>],
    capture_names: &[&'a str],
) -> (Option<Node<'a>>, Option<Node<'a>>, &'a str) {
    let mut decl = None;
    let mut name = None;
    let mut tag = "";

    for cap in captures {
        let cname = capture_names[cap.index as usize];
        if cname == "name" {
            name = Some(cap.node);
        } else {
            decl = Some(cap.node);
            tag = cname;
        }
    }
    (decl, name, tag)
}

pub fn compute_depth(node: Node) -> usize {
    let mut depth = 0;
    let mut curr = node.parent();
    while let Some(parent) = curr {
        if CONTAINER_KINDS.contains(&parent.kind()) {
            depth += 1;
        }
        curr = parent.parent();
    }
    depth
}

#[cfg(test)]
#[path = "parser/tests.rs"]
mod tests;
