use tree_sitter::Node;

use super::types::SymbolKind;

pub struct SymbolCandidate<'a> {
    pub tag: &'a str,
    pub decl: Node<'a>,
    pub name_node: Option<Node<'a>>,
}

fn resolve_candidate_name(candidate: &SymbolCandidate<'_>, source: &str) -> String {
    candidate
        .name_node
        .map(|n| extract_identifier_name(n, source))
        .filter(|s| !s.is_empty())
        .or_else(|| {
            candidate
                .decl
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "<anonymous>".to_string())
}

fn classify_type_kind(decl: Node<'_>) -> SymbolKind {
    if let Some(type_child) = decl.child_by_field_name("type") {
        if type_child.kind() == "struct_type" {
            return SymbolKind::Struct;
        }
        if type_child.kind() == "interface_type" {
            return SymbolKind::Interface;
        }
    }
    SymbolKind::Type
}

fn classify_tag(tag: &str, decl: Node<'_>) -> SymbolKind {
    match tag {
        "function" => {
            if is_method(decl) {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            }
        }
        "method" => SymbolKind::Method,
        "struct" => SymbolKind::Struct,
        "class" => SymbolKind::Class,
        "interface" => SymbolKind::Interface,
        "trait" => SymbolKind::Trait,
        "enum" => SymbolKind::Enum,
        "type" => classify_type_kind(decl),
        _ => SymbolKind::Function,
    }
}

pub fn classify_kind_and_name(candidate: SymbolCandidate<'_>, source: &str) -> (SymbolKind, String) {
    if candidate.tag == "impl" {
        return (SymbolKind::Impl, extract_impl_name(candidate.decl, source));
    }
    let name = resolve_candidate_name(&candidate, source);
    let kind = classify_tag(candidate.tag, candidate.decl);
    (kind, name)
}

fn extract_identifier_name(node: Node, source: &str) -> String {
    if node.kind() == "function_declarator" || node.kind() == "pointer_declarator" {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if (child.kind() == "identifier" || child.kind() == "field_identifier")
                && let Ok(text) = child.utf8_text(source.as_bytes())
            {
                return text.to_string();
            }
            if child.kind() == "function_declarator" || child.kind() == "pointer_declarator" {
                return extract_identifier_name(child, source);
            }
        }
    }
    node.utf8_text(source.as_bytes()).unwrap_or("").to_string()
}

fn extract_impl_name(node: Node, source: &str) -> String {
    if let Some(text) = node
        .child_by_field_name("type")
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
    {
        return text.to_string();
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "type_identifier"
            && let Ok(text) = child.utf8_text(source.as_bytes())
        {
            return text.to_string();
        }
    }
    "impl".to_string()
}

fn is_method(node: Node) -> bool {
    let mut curr = node.parent();
    while let Some(parent) = curr {
        if parent.kind() == "module" && parent.parent().is_none() {
            curr = parent.parent();
            continue;
        }
        match parent.kind() {
            "impl_item" | "trait_item" | "class_declaration" | "class_definition" | "class_specifier"
            | "record_declaration" | "class" | "module" => {
                return true;
            }
            "function_item" | "function_definition" | "function_declaration" => {
                return false;
            }
            _ => {}
        }
        curr = parent.parent();
    }
    false
}
