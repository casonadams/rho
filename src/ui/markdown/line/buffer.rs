//! Line prefix buffering heuristics and spacing decisions.

use super::blocks::is_horizontal_rule;

pub fn is_ordered_list_prefix_or_item(trimmed: &str) -> bool {
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let after = &trimmed[digits..];
        if after == "." || after.starts_with(". ") {
            return true;
        }
    }
    false
}

pub fn should_buffer_line(current_line: &str) -> bool {
    let trimmed = current_line.trim_start();
    trimmed.starts_with('|')
        || trimmed.starts_with('#')
        || trimmed.starts_with('`')
        || trimmed.starts_with('>')
        || trimmed == "-"
        || trimmed.starts_with("- ")
        || trimmed.starts_with("---")
        || trimmed == "*"
        || trimmed.starts_with("* ")
        || trimmed.starts_with("***")
        || trimmed.starts_with("___")
        || is_ordered_list_prefix_or_item(trimmed)
}

pub fn needs_preceding_blank_line(trimmed: &str, in_code_block: bool) -> bool {
    trimmed.starts_with('#')
        || (trimmed.starts_with("```") && !in_code_block)
        || trimmed.starts_with('>')
        || is_horizontal_rule(trimmed)
}
