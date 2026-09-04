use chrono::{DateTime, Utc};
use rig::message::Message;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

use super::compaction::CompactionMetadata;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TreeNodeKind {
    UserTurn,
    AssistantTurn,
    Compaction,
    BranchSummary,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TreeNodeData {
    pub id: String,
    pub parent_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub kind: TreeNodeKind,
    pub messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl TreeNodeData {
    pub fn compaction_metadata(&self) -> Option<CompactionMetadata> {
        if self.kind != TreeNodeKind::Compaction {
            return None;
        }
        self.metadata
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionTree {
    pub nodes: BTreeMap<String, TreeNodeData>,
    pub children: HashMap<Option<String>, Vec<String>>,
    pub active_leaf_id: Option<String>,
    pub session_name: Option<String>,
}

impl SessionTree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: TreeNodeData) {
        let node_id = node.id.clone();
        let parent_id = node.parent_id.clone();
        self.nodes.insert(node_id.clone(), node);
        self.children.entry(parent_id).or_default().push(node_id.clone());
        self.active_leaf_id = Some(node_id);
    }

    pub fn set_active_leaf(&mut self, leaf_id: Option<String>) {
        self.active_leaf_id = leaf_id;
    }

    pub fn set_node_label(&mut self, node_id: &str, label: Option<String>) -> bool {
        if let Some(node) = self.nodes.get_mut(node_id) {
            node.label = label;
            true
        } else {
            false
        }
    }

    pub fn set_session_name(&mut self, name: String) {
        self.session_name = Some(name);
    }

    pub fn get_node(&self, id: &str) -> Option<&TreeNodeData> {
        self.nodes.get(id)
    }

    pub fn root_nodes(&self) -> Vec<&TreeNodeData> {
        self.children_of(None)
    }

    pub fn children_of(&self, parent_id: Option<&str>) -> Vec<&TreeNodeData> {
        let key = parent_id.map(str::to_string);
        match self.children.get(&key) {
            Some(ids) => ids.iter().filter_map(|id| self.nodes.get(id)).collect(),
            None => Vec::new(),
        }
    }

    pub fn ancestor_nodes(&self, leaf_id: &str) -> Vec<&TreeNodeData> {
        let mut path = Vec::new();
        let mut current_id = Some(leaf_id.to_string());
        let mut visited = HashSet::new();

        while let Some(id) = current_id {
            if !visited.insert(id.clone()) {
                break;
            }
            if let Some(node) = self.nodes.get(&id) {
                path.push(node);
                current_id = node.parent_id.clone();
            } else {
                break;
            }
        }
        path.reverse();
        path
    }

    pub fn ancestor_messages(&self, leaf_id: &str) -> Vec<Message> {
        let nodes = self.ancestor_nodes(leaf_id);
        let Some(compaction_idx) = nodes.iter().rposition(|n| n.kind == TreeNodeKind::Compaction) else {
            return nodes.into_iter().flat_map(|node| node.messages.clone()).collect();
        };

        let compaction_node = nodes[compaction_idx];
        let metadata = compaction_node.compaction_metadata();
        let summary_message = if !compaction_node.messages.is_empty() {
            compaction_node.messages.clone()
        } else if let Some(ref meta) = metadata {
            vec![Message::System {
                content: meta.summary.clone(),
            }]
        } else if let Some(summary) = compaction_node
            .metadata
            .as_ref()
            .and_then(|v| v.get("summary").and_then(|s| s.as_str()))
        {
            vec![Message::System {
                content: summary.to_string(),
            }]
        } else {
            Vec::new()
        };

        let first_kept_idx = metadata
            .as_ref()
            .and_then(|m| m.first_kept_node_id.as_deref())
            .or_else(|| {
                compaction_node
                    .metadata
                    .as_ref()
                    .and_then(|v| v.get("first_kept_node_id").and_then(|s| s.as_str()))
            })
            .and_then(|id| nodes.iter().position(|n| n.id == id));

        let start_idx = first_kept_idx.unwrap_or(compaction_idx + 1);
        let mut messages = summary_message;
        for node in &nodes[start_idx..] {
            if node.kind != TreeNodeKind::Compaction {
                messages.extend(node.messages.clone());
            }
        }
        messages
    }

    pub fn active_messages(&self) -> Vec<Message> {
        match &self.active_leaf_id {
            Some(leaf_id) => self.ancestor_messages(leaf_id),
            None => Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn branch_divergence<'a>(
        &'a self,
        source_leaf_id: &str,
        target_leaf_id: &str,
    ) -> (Vec<&'a TreeNodeData>, Vec<&'a TreeNodeData>) {
        let source_path = self.ancestor_nodes(source_leaf_id);
        let target_path = self.ancestor_nodes(target_leaf_id);

        let source_ids: HashSet<&str> = source_path.iter().map(|n| n.id.as_str()).collect();
        let target_ids: HashSet<&str> = target_path.iter().map(|n| n.id.as_str()).collect();

        let unique_to_source: Vec<&'a TreeNodeData> = source_path
            .into_iter()
            .filter(|n| !target_ids.contains(n.id.as_str()))
            .collect();
        let unique_to_target: Vec<&'a TreeNodeData> = target_path
            .into_iter()
            .filter(|n| !source_ids.contains(n.id.as_str()))
            .collect();

        (unique_to_source, unique_to_target)
    }
}
