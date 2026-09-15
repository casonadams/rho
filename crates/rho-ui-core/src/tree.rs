use std::collections::HashMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeEntryDisplay {
    pub id: String,
    pub parent_id: Option<String>,
    pub depth: usize,
    pub is_last_child: bool,
    pub is_active: bool,
    pub label: Option<String>,
    pub kind: String,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeItemInput {
    pub id: String,
    pub parent_id: Option<String>,
    pub label: Option<String>,
    pub kind: String,
    pub preview: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, Copy)]
struct NodeRenderContext {
    depth: usize,
    is_last: bool,
}

pub fn build_tree_display(items: &[TreeItemInput]) -> Vec<TreeEntryDisplay> {
    let mut children_map: HashMap<Option<String>, Vec<&TreeItemInput>> = HashMap::new();
    let mut existing_ids = std::collections::HashSet::new();
    for item in items {
        existing_ids.insert(&item.id);
    }

    for item in items {
        let parent_key = match &item.parent_id {
            Some(pid) if existing_ids.contains(pid) => Some(pid.clone()),
            _ => None,
        };
        children_map.entry(parent_key).or_default().push(item);
    }

    let mut entries = Vec::with_capacity(items.len());
    let roots = children_map.remove(&None).unwrap_or_default();
    let root_count = roots.len();
    for (idx, root) in roots.into_iter().enumerate() {
        let ctx = NodeRenderContext {
            depth: 0,
            is_last: idx + 1 == root_count,
        };
        visit_node(root, ctx, &children_map, &mut entries);
    }

    entries
}

fn visit_node(
    item: &TreeItemInput,
    ctx: NodeRenderContext,
    children_map: &HashMap<Option<String>, Vec<&TreeItemInput>>,
    entries: &mut Vec<TreeEntryDisplay>,
) {
    entries.push(TreeEntryDisplay {
        id: item.id.clone(),
        parent_id: item.parent_id.clone(),
        depth: ctx.depth,
        is_last_child: ctx.is_last,
        is_active: item.is_active,
        label: item.label.clone(),
        kind: item.kind.clone(),
        preview: item.preview.clone(),
    });

    if let Some(children) = children_map.get(&Some(item.id.clone())) {
        let count = children.len();
        for (idx, child) in children.iter().enumerate() {
            let child_ctx = NodeRenderContext {
                depth: ctx.depth + 1,
                is_last: idx + 1 == count,
            };
            visit_node(child, child_ctx, children_map, entries);
        }
    }
}

pub fn render_tree_ascii(entries: &[TreeEntryDisplay]) -> String {
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
        let label_tag = entry.label.as_ref().map(|l| format!(" [{l}]")).unwrap_or_default();
        let short_id = &entry.id[..entry.id.floor_char_boundary(8.min(entry.id.len()))];
        let _ = writeln!(
            out,
            "  {indent}{branch_char}{}{label_tag}{active_tag} ({short_id})",
            entry.preview
        );
    }
    out
}
