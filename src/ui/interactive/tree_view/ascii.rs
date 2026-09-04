use std::fmt::Write as _;

use rho_harness_core::session::tree::SessionTree;

use super::entry::build_tree_display;

pub fn render_tree_ascii(tree: &SessionTree) -> String {
    let entries = build_tree_display(tree);
    if entries.is_empty() {
        return String::from("  (No conversation tree nodes recorded yet)\n");
    }
    let mut out = String::new();
    for entry in entries {
        let indent = "  ".repeat(entry.depth);
        let branch_char = if entry.is_last_child {
            "└── "
        } else {
            "├── "
        };
        let active_tag = if entry.is_active { " [ACTIVE]" } else { "" };
        let label_tag = entry.label.map(|l| format!(" [{l}]")).unwrap_or_default();
        let short_id = &entry.id[..entry.id.floor_char_boundary(8.min(entry.id.len()))];
        let _ = writeln!(
            out,
            "  {indent}{branch_char}{}{label_tag}{active_tag} ({short_id})",
            entry.preview
        );
    }
    out
}
