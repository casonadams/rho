use rho_harness_core::session::tree::{SessionTree, TreeNodeData, TreeNodeKind};

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

struct TreeDisplayBuilder<'a> {
    tree: &'a SessionTree,
    entries: Vec<TreeEntryDisplay>,
}

impl<'a> TreeDisplayBuilder<'a> {
    fn visit(&mut self, node: &TreeNodeData, ctx: NodeRenderContext) {
        let is_active = self.tree.active_leaf_id.as_deref() == Some(&node.id);
        let preview = match &node.kind {
            TreeNodeKind::UserTurn => {
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
            TreeNodeKind::AssistantTurn => {
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
            TreeNodeKind::BranchSummary => {
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
            TreeNodeKind::Compaction => "Compaction Checkpoint".to_string(),
            TreeNodeKind::Custom => "Custom".to_string(),
        };

        self.entries.push(TreeEntryDisplay {
            id: node.id.clone(),
            parent_id: node.parent_id.clone(),
            depth: ctx.depth,
            is_last_child: ctx.is_last,
            is_active,
            label: node.label.clone(),
            kind: node.kind.clone(),
            preview,
        });

        let children = self.tree.children_of(Some(&node.id));
        let child_count = children.len();
        for (idx, child) in children.iter().enumerate() {
            let child_ctx = NodeRenderContext {
                depth: ctx.depth + 1,
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
