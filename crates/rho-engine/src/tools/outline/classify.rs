use tree_sitter::Node;

use super::types::SymbolKind;

pub struct SymbolCandidate<'a> {
    pub tag: &'a str,
    pub decl: Node<'a>,
    pub name_node: Option<Node<'a>>,
}

pub fn classify_kind_and_name(candidate: SymbolCandidate<'_>, source: &str) -> (SymbolKind, String) {
    if candidate.tag == "impl" {
        return (SymbolKind::Impl, extract_impl_name(candidate.decl, source));
    }

    let name = candidate
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
        .unwrap_or_else(|| "<anonymous>".to_string());

    match candidate.tag {
        "function" => {
            if is_method(candidate.decl) {
                (SymbolKind::Method, name)
            } else {
                (SymbolKind::Function, name)
            }
        }
        "method" => (SymbolKind::Method, name),
        "struct" => (SymbolKind::Struct, name),
        "class" => (SymbolKind::Class, name),
        "interface" => (SymbolKind::Interface, name),
        "trait" => (SymbolKind::Trait, name),
        "enum" => (SymbolKind::Enum, name),
        "type" => {
            if let Some(type_child) = candidate.decl.child_by_field_name("type") {
                if type_child.kind() == "struct_type" {
                    return (SymbolKind::Struct, name);
                }
                if type_child.kind() == "interface_type" {
                    return (SymbolKind::Interface, name);
                }
            }
            (SymbolKind::Type, name)
        }
        _ => (SymbolKind::Function, name),
    }
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
