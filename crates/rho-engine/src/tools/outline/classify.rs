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

    let raw_name = candidate
        .name_node
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        .unwrap_or("");
    let name = if raw_name.is_empty() {
        candidate
            .decl
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            .unwrap_or("<anonymous>")
            .to_string()
    } else {
        raw_name.to_string()
    };

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
        match parent.kind() {
            "impl_item" | "trait_item" | "class_declaration" | "class_definition" => {
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
