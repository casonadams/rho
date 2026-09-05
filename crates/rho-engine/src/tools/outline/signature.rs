use tree_sitter::Node;

const BODY_KINDS: &[&str] = &[
    "block",
    "statement_block",
    "compound_statement",
    "field_declaration_list",
    "enum_variant_list",
    "declaration_list",
    "class_body",
    "interface_body",
    "record_body",
    "constructor_body",
    "body_statement",
    "enum_body",
    "enumerator_list",
    "enum_member_declaration_list",
    "enum_declaration_list",
    "object_type",
];

pub fn extract_signature(node: Node, source: &str) -> (String, usize) {
    let (start_byte, line) = find_start_byte_and_line(node);
    let end_byte = find_signature_end(node);
    let raw = if end_byte > start_byte && end_byte <= source.len() {
        &source[start_byte..end_byte]
    } else {
        node.utf8_text(source.as_bytes()).unwrap_or("")
    };
    (normalize_signature(raw), line)
}

fn find_start_byte_and_line(node: Node) -> (usize, usize) {
    if let Some(parent) = node
        .parent()
        .filter(|p| p.kind() == "export_statement" || p.kind() == "type_declaration")
    {
        return (parent.start_byte(), parent.start_position().row + 1);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "attribute_item" {
            continue;
        }
        return (child.start_byte(), child.start_position().row + 1);
    }
    (node.start_byte(), node.start_position().row + 1)
}

fn find_signature_end(node: Node) -> usize {
    if let Some(body) = find_body_child(node) {
        return body.start_byte();
    }
    if let Some(parent) = node
        .parent()
        .filter(|p| p.kind() == "export_statement" || p.kind() == "type_declaration")
    {
        return parent.end_byte();
    }
    node.end_byte()
}

fn find_body_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if BODY_KINDS.contains(&child.kind()) {
            return Some(child);
        }
        if child.kind() == "struct_type" || child.kind() == "interface_type" {
            let mut inner = child.walk();
            for subchild in child.children(&mut inner) {
                if BODY_KINDS.contains(&subchild.kind()) || subchild.kind() == "{" {
                    return Some(subchild);
                }
            }
        }
    }
    None
}

pub fn normalize_signature(sig: &str) -> String {
    let raw = sig.trim_end_matches(|c: char| c == '{' || c == ';' || c.is_whitespace());
    let parts: Vec<&str> = raw.split_whitespace().collect();
    let joined = parts.join(" ");
    joined.replace("( ", "(").replace(" )", ")").replace(" ,", ",")
}
