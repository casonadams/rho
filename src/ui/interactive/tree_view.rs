use rho_harness_core::session::tree::{SessionTree, TreeNodeData, TreeNodeKind};
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub struct TreeEntryDisplay {
    pub id: String,
    pub parent_id: Option<String>,
    pub depth: usize,
    pub is_last_child: bool,
    pub is_active: bool,
    pub label: Option<String>,
    pub kind: TreeNodeKind,
    pub preview: String,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct NodeRenderContext {
    pub depth: usize,
    pub is_last: bool,
}

pub fn build_tree_display(tree: &SessionTree) -> Vec<TreeEntryDisplay> {
    let mut builder = TreeDisplayBuilder {
        tree,
        entries: Vec::new(),
    };
    let roots = tree.root_nodes();
    let total = roots.len();
    for (idx, root) in roots.iter().enumerate() {
        let ctx = NodeRenderContext {
            depth: 0,
            is_last: idx + 1 == total,
        };
        builder.visit(root, ctx);
    }
    builder.entries
}

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

struct TreeDisplayBuilder<'a> {
    tree: &'a SessionTree,
    entries: Vec<TreeEntryDisplay>,
}

fn user_preview(node: &TreeNodeData) -> String {
    let text = node
        .messages
        .iter()
        .find_map(|m| match m {
            rig::message::Message::User { content } => content.first().map(|c| match c {
                rig::message::UserContent::Text(t) => t.text.clone(),
                _ => format!("{:?}", c),
            }),
            _ => None,
        })
        .unwrap_or_default();
    format!("User: \"{}\"", truncate_preview(&text, 45))
}

fn assistant_preview(node: &TreeNodeData) -> String {
    let text = node
        .messages
        .iter()
        .find_map(|m| match m {
            rig::message::Message::Assistant { content, .. } => content.first().map(|c| match c {
                rig::message::AssistantContent::Text(t) => t.text.clone(),
                _ => format!("{:?}", c),
            }),
            _ => None,
        })
        .unwrap_or_default();
    format!("Assistant: \"{}\"", truncate_preview(&text, 45))
}

fn summary_preview(node: &TreeNodeData) -> String {
    let text = node
        .messages
        .first()
        .map(|m| match m {
            rig::message::Message::Assistant { content, .. } => content
                .first()
                .map(|c| match c {
                    rig::message::AssistantContent::Text(t) => t.text.clone(),
                    _ => format!("{:?}", c),
                })
                .unwrap_or_default(),
            _ => format!("{:?}", m),
        })
        .unwrap_or_default();
    format!("Summary: \"{}\"", truncate_preview(&text, 45))
}

fn node_preview(node: &TreeNodeData) -> String {
    match &node.kind {
        TreeNodeKind::UserTurn => user_preview(node),
        TreeNodeKind::AssistantTurn => assistant_preview(node),
        TreeNodeKind::BranchSummary => summary_preview(node),
        TreeNodeKind::Compaction => "Compaction Checkpoint".to_string(),
        TreeNodeKind::Custom => "Custom".to_string(),
    }
}

impl<'a> TreeDisplayBuilder<'a> {
    fn visit(&mut self, node: &TreeNodeData, ctx: NodeRenderContext) {
        let is_active = self.tree.active_leaf_id.as_deref() == Some(&node.id);
        self.entries.push(TreeEntryDisplay {
            id: node.id.clone(),
            parent_id: node.parent_id.clone(),
            depth: ctx.depth,
            is_last_child: ctx.is_last,
            is_active,
            label: node.label.clone(),
            kind: node.kind.clone(),
            preview: node_preview(node),
        });
        self.visit_children(&node.id, ctx.depth);
    }

    fn visit_children(&mut self, node_id: &str, depth: usize) {
        let children = self.tree.children_of(Some(node_id));
        let child_count = children.len();
        for (idx, child) in children.iter().enumerate() {
            let child_ctx = NodeRenderContext {
                depth: depth + 1,
                is_last: idx + 1 == child_count,
            };
            self.visit(child, child_ctx);
        }
    }
}

fn truncate_preview(text: &str, limit: usize) -> String {
    let text = text.replace('\n', " ").trim().to_string();
    if text.chars().count() > limit {
        format!("{}...", text.chars().take(limit.saturating_sub(3)).collect::<String>())
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rig::message::Message;

    fn sample_two_node_tree() -> SessionTree {
        let mut tree = SessionTree::new();
        tree.add_node(TreeNodeData {
            id: "root-1".to_string(),
            parent_id: None,
            timestamp: Utc::now(),
            kind: TreeNodeKind::UserTurn,
            messages: vec![Message::user("Root prompt")],
            label: Some("root".to_string()),
            metadata: None,
        });
        tree.add_node(TreeNodeData {
            id: "child-1".to_string(),
            parent_id: Some("root-1".to_string()),
            timestamp: Utc::now(),
            kind: TreeNodeKind::AssistantTurn,
            messages: vec![Message::assistant("Child answer")],
            label: None,
            metadata: None,
        });
        tree
    }

    #[test]
    fn test_tree_display_hierarchy() {
        let tree = sample_two_node_tree();
        let display = build_tree_display(&tree);
        assert_eq!(display.len(), 2);
        assert_eq!((display[0].depth, display[0].label.as_deref()), (0, Some("root")));
        assert_eq!((display[1].depth, display[1].is_active), (1, true));
    }

    #[test]
    fn test_tree_display_ascii() {
        let tree = sample_two_node_tree();
        let ascii = render_tree_ascii(&tree);
        assert!(ascii.contains("User: \"Root prompt\""));
        assert!(ascii.contains("Assistant: \"Child answer\"") && ascii.contains("[ACTIVE]"));
    }
}
